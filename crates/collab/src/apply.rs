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
    CellBody, ChangeKind, Delta, FormatAddress, FormatCmd, HistoryAction, MediaKind, OpEnvelope, OpKind, Payload, RejectCode, ShapeField, ShapeKey,
    SlideKey, TableAxis, Target, TextRange,
};
use deckcraft_engine::{Selection, Session, Target as EngineTarget, TextSel};
use deckcraft_model::text::TextBody;
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind, SlideId, Table, find_shape, parent_of};
use serde_json::{Value, json};

use crate::delta::delta;
use crate::history::{
    Change, FontImage, GroupImage, Histories, Image, NotesImage, Restore, ShapeImage, SlideAnimationsImage, SlideImage, Snapshot, Step,
    TransitionImage,
};
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

/// Most bodies one `format.*` operation may address: a whole table selection stays far below it,
/// and the operation keeps one message and one undo step small.
const MAX_FORMAT_ADDRESSES: usize = 4096;

/// Most cell bodies one `cells_text` restore may carry, and the largest combined JSON it may
/// hold (the same ceiling a group restore uses), so one undo stays inside the wire limit.
const MAX_CELL_BODIES: usize = 4096;
const MAX_CELL_BODIES_JSON: usize = 192 * 1024;

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
                Payload::ShapeField { .. }
                    | Payload::CellText { .. }
                    | Payload::CellsText { .. }
                    | Payload::ShapeFields { .. }
                    | Payload::ShapeRestore { .. }
                    | Payload::SlideRestore { .. }
                    | Payload::FontRestore { .. }
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
            target: Target { slide, shape, shapes, ..Target::default() },
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
            target: Target { slide, shape, shapes, ..Target::default() },
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
                if op.target.slide == expected.slide
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
    /// slide, which is what structural undo restores (RFC §13, item 4). A transition step records
    /// the slide's whole transition. A shape insert (preset, picture, media) and `edit.delete`
    /// record the whole shape with its place in the z-order and its animations (RFC §6).
    fn capture_step(&self, before: &Presentation, after: &Presentation, op: &OpEnvelope) -> Option<(Vec<Change>, Vec<Change>)> {
        // A notes step is a slide body, not a shape value: the whole body before and after is the
        // image, so an undo puts back the runs the typed string cannot describe.
        if matches!(op.payload, Payload::NotesSet { .. } | Payload::NotesInsert { .. } | Payload::NotesDelete { .. }) {
            let key = op.target.slide.clone()?;
            let was = self.capture_notes(before, &key)?;
            let now = self.capture_notes(after, &key)?;
            return Some((vec![Change::Notes(was)], vec![Change::Notes(now)]));
        }
        let (cell, image) = match &op.payload {
            Payload::ShapeMove { .. } | Payload::ShapeSetBounds { .. } => (None, Image::Xfrm { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }),
            Payload::ShapesSetBounds { items } => return self.capture_bounds_step(before, after, op, items),
            Payload::TextSet { cell, .. } => (*cell, Image::Text(String::new())),
            Payload::TextInsert { cell, .. } | Payload::TextDelete { cell, .. } => (*cell, Image::Text(String::new())),
            // A table is one value: a structural edit changes the cell indices and values, a
            // style, a cell fill, a merge/split or a column width/row height changes the table
            // without naming a cell that could be restored on its own, so the whole table is the
            // exact before/after image of every one of these steps (RFC §15).
            Payload::TableEdit { .. }
            | Payload::TableStyle { .. }
            | Payload::TableCellFill { .. }
            | Payload::TableMerge { .. }
            | Payload::TableSplit { .. }
            | Payload::TableColumnWidth { .. }
            | Payload::TableRowHeight { .. }
            | Payload::TableOptions { .. } => (None, Image::Table(deckcraft_model::Table::default())),
            // A SmartArt edit replaces the whole graphic: the outline, layout, style and colours
            // are one value, and the generated shapes are replaced with it.
            Payload::SmartArtSet { .. } => (None, Image::SmartArt(Value::Null)),
            // A chart edit replaces the chart's value, and the exact image of the step is the whole
            // shape: the user-facing chart commands carry only part of the model and a kept chart
            // part cannot be replayed from them.
            Payload::ChartSet { .. } => (None, Image::Chart(Value::Null)),
            Payload::ShapeFill { .. } => return self.capture_appearance(before, after, op, Image::Fill(None)),
            Payload::ShapeLine { .. } => return self.capture_appearance(before, after, op, Image::Line(None)),
            Payload::SlideTransition { .. } => return self.capture_transition_step(before, after, op),
            Payload::ShapeAnimations { .. } => (None, Image::Animation(Vec::new())),
            Payload::SlideAnimations { .. } => return self.capture_animations_step(before, after, op),
            Payload::FormatSet { addresses, .. } => return self.capture_format_step(before, after, op, addresses),
            Payload::FontCreate { .. } | Payload::FontSet { .. } | Payload::FontRemove { .. } => {
                let typeface = match &op.payload {
                    Payload::FontCreate { typeface, .. } | Payload::FontSet { typeface, .. } | Payload::FontRemove { typeface } => typeface,
                    _ => return None,
                };
                return Some((vec![Change::Font(capture_font(before, typeface))], vec![Change::Font(capture_font(after, typeface))]));
            }
            Payload::ShapeField { field, .. } => (None, image_of_field(*field)),
            Payload::ShapeInsert { .. } | Payload::PictureInsert { .. } | Payload::MediaInsert { .. } => {
                return self.capture_shape_insert(before, after, op);
            }
            Payload::ChartInsert { .. } => return self.capture_shape_insert(before, after, op),
            Payload::SmartArtInsert { .. } => return self.capture_shape_insert(before, after, op),
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

    /// Undo of a `format.*` step over N bodies: one whole-body image per addressed body (a table
    /// cell or the shape's own body), so the restore puts back exactly those bodies and no
    /// neighbour. A body that cannot be captured makes the whole step unrecordable.
    fn capture_format_step(
        &self,
        before: &Presentation,
        after: &Presentation,
        op: &OpEnvelope,
        addresses: &[FormatAddress],
    ) -> Option<(Vec<Change>, Vec<Change>)> {
        let slide = op.target.slide.clone()?;
        let shape = op.target.shape.clone()?;
        let mut was = Vec::with_capacity(addresses.len());
        let mut now = Vec::with_capacity(addresses.len());
        for address in addresses {
            let template = Snapshot { slide: slide.clone(), shape: shape.clone(), cell: address.cell, image: Image::Format(TextBody::default()) };
            was.push(Change::Shape(self.capture_snapshot(before, &template)?));
            now.push(Change::Shape(self.capture_snapshot(after, &template)?));
        }
        Some((was, now))
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

    /// Undo of `shape.insert`, `insert.picture`, `insert.video`/`insert.audio` or
    /// `insert.model3d`: the one top-most shape the step added, with its z-order and animations.
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

    /// Undo of an `animation.set`/`animation.timing`/`animation.move` step: the slide's whole
    /// ordered animation list before and after. A step may move past another shape's step, so a
    /// per-shape image could not put the sequence back; the whole list can.
    fn capture_animations_step(&self, before: &Presentation, after: &Presentation, op: &OpEnvelope) -> Option<(Vec<Change>, Vec<Change>)> {
        let key = op.target.slide.clone()?;
        let was = self.capture_animations(before, &key)?;
        let now = self.capture_animations(after, &key)?;
        Some((vec![Change::SlideAnimations(was)], vec![Change::SlideAnimations(now)]))
    }

    /// One slide's whole animation sequence as an image, addressed by slide key.
    fn capture_animations(&self, doc: &Presentation, slide: &SlideKey) -> Option<SlideAnimationsImage> {
        let id = self.keys.slide_id(slide)?;
        let found = doc.slide(id)?;
        Some(SlideAnimationsImage { slide: slide.clone(), animations: found.animations.clone() })
    }

    /// Undo of a `transition.set` step: the slide's whole transition before and after.
    fn capture_transition_step(&self, before: &Presentation, after: &Presentation, op: &OpEnvelope) -> Option<(Vec<Change>, Vec<Change>)> {
        let key = op.target.slide.clone()?;
        let id = self.keys.slide_id(&key)?;
        let was = before.slide(id)?.transition.clone();
        let now = after.slide(id)?.transition.clone();
        Some((
            vec![Change::SlideTransition(TransitionImage { slide: key.clone(), transition: was })],
            vec![Change::SlideTransition(TransitionImage { slide: key, transition: now })],
        ))
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
            Change::SlideTransition(snapshot) => {
                let id = self.keys.slide_id(&snapshot.slide)?;
                let slide = doc.slide(id)?;
                Some(Change::SlideTransition(TransitionImage { slide: snapshot.slide.clone(), transition: slide.transition.clone() }))
            }
            Change::Notes(snapshot) => self.capture_notes(doc, &snapshot.slide).map(Change::Notes),
            Change::SlideAnimations(snapshot) => self.capture_animations(doc, &snapshot.slide).map(Change::SlideAnimations),
            Change::Font(snapshot) => Some(Change::Font(capture_font(doc, &snapshot.typeface))),
        }
    }

    /// The slide's speaker notes as a whole-body image, addressed by slide key.
    fn capture_notes(&self, doc: &Presentation, slide: &SlideKey) -> Option<NotesImage> {
        let id = self.keys.slide_id(slide)?;
        let found = doc.slide(id)?;
        Some(NotesImage { slide: slide.clone(), notes: found.notes.clone() })
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
            // A format step records the body it replaced: the shape's whole body, or the whole
            // body of the one cell it addressed (a table's own `text` is empty).
            Image::Format(_) => match (&shape.kind, at.cell) {
                (ShapeKind::Table(t), Some((r, c))) => Image::Format(t.cell(r as usize, c as usize)?.text.clone()),
                (ShapeKind::Table(_), None) => return None,
                _ => Image::Format(shape.text.clone()?),
            },
            Image::Table(_) => match &shape.kind {
                ShapeKind::Table(t) => Image::Table(t.clone()),
                _ => return None,
            },
            // A SmartArt group is one value: its diagram and the shapes generated from it. The
            // whole model keeps the children's ids and any imported file drawing, which the
            // diagram value alone cannot reproduce.
            Image::SmartArt(_) => Image::SmartArt(serde_json::to_value(shape).ok()?),
            // A chart step records the whole shape: its box and its chart value (type, data,
            // options and kept parts).
            Image::Chart(_) => Image::Chart(serde_json::to_value(shape).ok()?),
            Image::Animation(_) => {
                let slide = doc.slide(slide_id)?;
                Image::Animation(slide.animations.iter().filter(|a| a.shape == shape_id).cloned().collect())
            }
            // Notes are a slide body, never a shape value: they have their own `Change::Notes`.
            Image::Notes(_) => return None,
            // A slide-wide animation list is a slide body too, never a shape value.
            Image::SlideAnimations(_) => return None,
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
            Payload::TextInsert { at, text, cell } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                let body = self.text_body(before, slide, shape, *cell, head)?;
                let pos = offset_position(body, *at as usize);
                let sel = TextSel { shape, anchor: pos, caret: pos, cell: cell.map(|(r, c)| (r as usize, c as usize)), notes: false, line_end: None };
                self.select_text(session, before, slide, sel, head)?;
                self.call(session, "text.insert", json!({"text": text}), head)
            }
            Payload::TextDelete { from, to, cell } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                if to <= from {
                    return Err(Rejection::invalid("the deleted range is empty", Some(head)));
                }
                let body = self.text_body(before, slide, shape, *cell, head)?;
                let start = offset_position(body, *from as usize);
                let end = offset_position(body, *to as usize);
                let sel =
                    TextSel { shape, anchor: start, caret: end, cell: cell.map(|(r, c)| (r as usize, c as usize)), notes: false, line_end: None };
                self.select_text(session, before, slide, sel, head)?;
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
            Payload::ChartInsert { chart, rect } => {
                let slide = self.resolve_slide(before, op.target.slide.as_ref(), head)?;
                // The whole chart is the author's, so the operation carries exactly the chart its
                // command would leave; the size bound applies before the first write.
                let bytes = serde_json::to_vec(chart).map_err(|e| Rejection::invalid(format!("invalid chart: {e}"), Some(head)))?;
                if bytes.len() > MAX_CHART_JSON {
                    return Err(Rejection::invalid("the chart is too large", Some(head)));
                }
                self.select_slide(session, before, slide, head)?;
                let mut args = json!({"chart": chart});
                if let Some(rect) = rect {
                    args["rect"] = json!(rect);
                }
                self.call(session, "insert.chart", args, head)
            }
            Payload::SmartArtInsert { diagram, rect } => {
                let slide = self.resolve_slide(before, op.target.slide.as_ref(), head)?;
                // The whole diagram is the author's, so the operation carries exactly the graphic
                // its command would leave; the bounds apply before the first write, so a hostile
                // tree adds nothing and moves no version.
                let bytes = serde_json::to_vec(diagram).map_err(|e| Rejection::invalid(format!("invalid diagram: {e}"), Some(head)))?;
                if bytes.len() > MAX_SMARTART_JSON {
                    return Err(Rejection::invalid("the diagram is too large", Some(head)));
                }
                self.select_slide(session, before, slide, head)?;
                let mut args = json!({"diagram": diagram});
                if let Some(rect) = rect {
                    args["rect"] = json!(rect);
                }
                self.call(session, "insert.smartArt", args, head)
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
                    MediaKind::Model => crate::MediaType::Model,
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
                    MediaKind::Model => "insert.model3d",
                };
                self.call(session, cmd, args, head)
            }
            Payload::FontCreate { typeface, face, sha256, name } => {
                // A document font is presentation-level: no slide is named, so nothing is
                // selected and the operation applies whatever slide the replica is looking at.
                if op.target.slide.is_some() || op.target.shape.is_some() || !op.target.shapes.is_empty() {
                    return Err(Rejection::invalid("a document font belongs to the presentation, not to a slide or a shape", Some(head)));
                }
                let bytes = self
                    .media
                    .get(sha256)
                    .ok_or_else(|| Rejection::missing_media(format!("font `{sha256}` has not been uploaded to this session"), Some(head)))?;
                if let Some(kind) = self.media.kind(sha256)
                    && kind != crate::MediaType::Font
                {
                    return Err(Rejection::invalid(format!("`{sha256}` is a {} object, not a font", kind.noun()), Some(head)));
                }
                self.call(
                    session,
                    "font.create",
                    json!({
                        "typeface": typeface,
                        "face": face.as_str(),
                        "name": name,
                        "data": base64::engine::general_purpose::STANDARD.encode(bytes.as_slice()),
                    }),
                    head,
                )
            }
            Payload::FontSet { typeface, panose, pitch_family, charset } => {
                if op.target.slide.is_some() || op.target.shape.is_some() || !op.target.shapes.is_empty() {
                    return Err(Rejection::invalid("a document font belongs to the presentation, not to a slide or a shape", Some(head)));
                }
                let mut args = json!({"typeface": typeface});
                if let Some(value) = panose {
                    args["panose"] = json!(value);
                }
                if let Some(value) = pitch_family {
                    args["pitchFamily"] = json!(value);
                }
                if let Some(value) = charset {
                    args["charset"] = json!(value);
                }
                self.call(session, "font.set", args, head)
            }
            Payload::FontRemove { typeface } => {
                if op.target.slide.is_some() || op.target.shape.is_some() || !op.target.shapes.is_empty() {
                    return Err(Rejection::invalid("a document font belongs to the presentation, not to a slide or a shape", Some(head)));
                }
                self.call(session, "font.remove", json!({"typeface": typeface}), head)
            }
            Payload::FontRestore { typeface, font } => {
                if op.target.slide.is_some() || op.target.shape.is_some() || !op.target.shapes.is_empty() {
                    return Err(Rejection::invalid("a document font belongs to the presentation, not to a slide or a shape", Some(head)));
                }
                self.call(session, "font.restore", json!({"typeface": typeface, "font": font}), head)
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
            Payload::SlideTransition { transition } => {
                let slide = self.resolve_slide(before, op.target.slide.as_ref(), head)?;
                let index = before.slide_index(slide).ok_or_else(|| Rejection::stale("the slide is gone", Some(head)))?;
                let value = match transition {
                    Some(spec) => {
                        let value =
                            crate::transition::value(spec).ok_or_else(|| Rejection::invalid("the transition value is not valid", Some(head)))?;
                        serde_json::to_value(value).map_err(|_| Rejection::invalid("the transition value is not valid", Some(head)))?
                    }
                    None => Value::Null,
                };
                self.call(session, "slide.transition.set", json!({"index": index, "transition": value}), head)
            }
            Payload::ShapeAnimations { animations } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                let index = before.slide_index(slide).ok_or_else(|| Rejection::stale("the slide is gone", Some(head)))?;
                // Every value is a whole model animation, the same form the engine command
                // validates again. The addressed shape wins: a payload may not animate another
                // shape's steps. A value the model cannot read refuses the operation before the
                // first write, so a hostile list never leaves half a shape animated.
                let mut steps = Vec::with_capacity(animations.len());
                for value in animations {
                    let step = crate::animation::value(value, shape)
                        .ok_or_else(|| Rejection::invalid("an animation step is not a valid model animation", Some(head)))?;
                    let json = crate::animation::spec(&step).ok_or_else(|| Rejection::invalid("an animation step cannot be sent", Some(head)))?;
                    steps.push(json);
                }
                self.call(session, "slide.animation.set", json!({"index": index, "shape": shape.0, "animations": steps}), head)
            }
            Payload::SlideAnimations { animations } => {
                // The slide's own ordered sequence: every value carries its shape, because a step
                // may move past another shape's step. Nothing is written before every value is a
                // model animation; the engine validates effect, bounds and that each named shape
                // is still on the slide.
                let slide = self.resolve_slide(before, op.target.slide.as_ref(), head)?;
                if op.target.shape.is_some() || !op.target.shapes.is_empty() {
                    return Err(Rejection::invalid("a slide animation list belongs to a slide, not to a shape", Some(head)));
                }
                let index = before.slide_index(slide).ok_or_else(|| Rejection::stale("the slide is gone", Some(head)))?;
                let mut steps = Vec::with_capacity(animations.len());
                for value in animations {
                    let step = crate::animation::model(value)
                        .ok_or_else(|| Rejection::invalid("an animation step is not a valid model animation", Some(head)))?;
                    let json = crate::animation::spec(&step).ok_or_else(|| Rejection::invalid("an animation step cannot be sent", Some(head)))?;
                    steps.push(json);
                }
                self.call(session, "slide.animations.set", json!({"index": index, "animations": steps}), head)
            }
            Payload::NotesSet { notes } => {
                // Notes belong to a slide: `target.slide` is the whole address, and a shape key in
                // the target would mean the sender misspelt the object it edits.
                let slide = self.resolve_slide(before, op.target.slide.as_ref(), head)?;
                if op.target.shape.is_some() || !op.target.shapes.is_empty() {
                    return Err(Rejection::invalid("notes are a slide's own body, not a shape's", Some(head)));
                }
                let index = before.slide_index(slide).ok_or_else(|| Rejection::stale("the slide is gone", Some(head)))?;
                // The value is a whole model text body: a value this build cannot read refuses the
                // operation before the first write, exactly like `shape.setField`.
                serde_json::from_value::<TextBody>(notes.clone())
                    .map_err(|_| Rejection::invalid("the notes value is not a text body", Some(head)))?;
                self.call_notes(session, before, slide, None, NotesShift::Replace, "slide.notes", json!({"index": index, "body": notes}), head)
            }
            Payload::NotesInsert { at, text } => {
                let slide = self.resolve_slide(before, op.target.slide.as_ref(), head)?;
                if op.target.shape.is_some() || !op.target.shapes.is_empty() {
                    return Err(Rejection::invalid("notes are a slide's own body, not a shape's", Some(head)));
                }
                let notes = before.slide(slide).map(|s| &s.notes);
                let pos = offset_position(notes, *at as usize);
                let sel = TextSel { shape: ShapeId(0), anchor: pos, caret: pos, cell: None, notes: true, line_end: None };
                let shift = NotesShift::Insert { at: *at, len: text.chars().count() as u32 };
                self.call_notes(session, before, slide, Some(sel), shift, "text.insert", json!({"text": text}), head)
            }
            Payload::NotesDelete { from, to } => {
                let slide = self.resolve_slide(before, op.target.slide.as_ref(), head)?;
                if op.target.shape.is_some() || !op.target.shapes.is_empty() {
                    return Err(Rejection::invalid("notes are a slide's own body, not a shape's", Some(head)));
                }
                if to <= from {
                    return Err(Rejection::invalid("the deleted notes range is empty", Some(head)));
                }
                let notes = before.slide(slide).map(|s| &s.notes);
                let start = offset_position(notes, *from as usize);
                let end = offset_position(notes, *to as usize);
                let sel = TextSel { shape: ShapeId(0), anchor: start, caret: end, cell: None, notes: true, line_end: None };
                let shift = NotesShift::Delete { from: *from, to: *to };
                self.call_notes(session, before, slide, Some(sel), shift, "text.delete", json!({}), head)
            }
            Payload::FormatSet { format, args, addresses } => self.run_format(session, before, head, op, format, args, addresses),
            Payload::CellsText { cells } => self.run_cells_text(session, before, head, op, cells),
            Payload::TableEdit { axis, at, insert } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                self.table_of(before, slide, shape, head)?;
                let (cmd, key) = match (axis, insert) {
                    (TableAxis::Row, true) => ("table.insertRowAbove", "row"),
                    (TableAxis::Row, false) => ("table.deleteRow", "row"),
                    (TableAxis::Column, true) => ("table.insertColumnLeft", "col"),
                    (TableAxis::Column, false) => ("table.deleteColumn", "col"),
                };
                self.select_shapes(session, before, slide, &[shape], head)?;
                self.call(session, cmd, json!({"id": shape.0, key: *at}), head)
            }
            Payload::TableStyle { style } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                self.table_of(before, slide, shape, head)?;
                if style.is_empty() || style.chars().count() > 255 || style.chars().any(char::is_control) {
                    return Err(Rejection::invalid("the table style name is not usable", Some(head)));
                }
                self.select_shapes(session, before, slide, &[shape], head)?;
                self.call(session, "table.style", json!({"id": shape.0, "style": style}), head)
            }
            Payload::TableCellFill { fill, cells } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                let table = self.table_of(before, slide, shape, head)?;
                if cells.is_empty() {
                    return Err(Rejection::invalid("a cell fill names no cell", Some(head)));
                }
                if let Some((r, c)) = cells.iter().find(|(r, c)| *r as usize >= table.n_rows() || *c as usize >= table.n_cols()) {
                    return Err(Rejection::invalid(format!("the table has no cell ({r}, {c})"), Some(head)));
                }
                // The value is final and the cell list is explicit: the command writes exactly
                // those cells, so a neighbour and this replica's own selection never decide.
                let mut args = fill.args();
                args["id"] = json!(shape.0);
                args["cells"] = json!(cells);
                self.select_shapes(session, before, slide, &[shape], head)?;
                self.call(session, "table.cellFill", args, head)
            }
            Payload::TableMerge { from, to } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                let table = self.table_of(before, slide, shape, head)?;
                for (r, c) in [from, to] {
                    if *r as usize >= table.n_rows() || *c as usize >= table.n_cols() {
                        return Err(Rejection::invalid(format!("the table has no cell ({r}, {c})"), Some(head)));
                    }
                }
                if from == to {
                    return Err(Rejection::invalid("a merge needs at least two cells", Some(head)));
                }
                self.select_shapes(session, before, slide, &[shape], head)?;
                self.call(session, "table.merge", json!({"id": shape.0, "from": [from.0, from.1], "to": [to.0, to.1]}), head)
            }
            Payload::TableSplit { cell } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                let table = self.table_of(before, slide, shape, head)?;
                let (r, c) = (cell.0 as usize, cell.1 as usize);
                let Some(target) = table.cell(r, c) else {
                    return Err(Rejection::invalid(format!("the table has no cell ({}, {})", cell.0, cell.1), Some(head)));
                };
                if target.grid_span.max(1) == 1 && target.row_span.max(1) == 1 {
                    return Err(Rejection::invalid("that cell is not merged", Some(head)));
                }
                self.select_shapes(session, before, slide, &[shape], head)?;
                self.call(session, "table.split", json!({"id": shape.0, "cell": [cell.0, cell.1]}), head)
            }
            Payload::TableColumnWidth { col, width, keep_total } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                let table = self.table_of(before, slide, shape, head)?;
                if *col as usize >= table.n_cols() {
                    return Err(Rejection::invalid(format!("the table has no column {col}"), Some(head)));
                }
                if !width.is_finite() {
                    return Err(Rejection::invalid("the column width is not a finite number", Some(head)));
                }
                self.select_shapes(session, before, slide, &[shape], head)?;
                self.call(session, "table.columnWidth", json!({"id": shape.0, "col": col, "width": width, "keepTotal": keep_total}), head)
            }
            Payload::TableRowHeight { row, height } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                let table = self.table_of(before, slide, shape, head)?;
                if *row as usize >= table.n_rows() {
                    return Err(Rejection::invalid(format!("the table has no row {row}"), Some(head)));
                }
                if !height.is_finite() {
                    return Err(Rejection::invalid("the row height is not a finite number", Some(head)));
                }
                self.select_shapes(session, before, slide, &[shape], head)?;
                self.call(session, "table.rowHeight", json!({"id": shape.0, "row": row, "height": height}), head)
            }
            Payload::TableOptions { header_row, total_row, banded_rows, first_column, last_column, banded_columns } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                self.table_of(before, slide, shape, head)?;
                // All six are named, so the command applies one final set and no replica's own
                // copy decides an option the sender did not name.
                self.select_shapes(session, before, slide, &[shape], head)?;
                self.call(
                    session,
                    "table.options",
                    json!({
                        "id": shape.0,
                        "headerRow": header_row,
                        "totalRow": total_row,
                        "bandedRows": banded_rows,
                        "firstColumn": first_column,
                        "lastColumn": last_column,
                        "bandedColumns": banded_columns,
                    }),
                    head,
                )
            }
            Payload::SmartArtSet { diagram } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                // Address the group before the first write: an operation that names a shape which
                // is not (inside) a SmartArt graphic must not regenerate anything.
                let group = smartart_group(before, slide, shape);
                if group.is_none() {
                    return Err(Rejection::invalid("the shape is not a SmartArt graphic", Some(head)));
                }
                let bytes = serde_json::to_vec(diagram).map_err(|e| Rejection::invalid(format!("invalid diagram: {e}"), Some(head)))?;
                if bytes.len() > MAX_SMARTART_JSON {
                    return Err(Rejection::invalid("the diagram is too large", Some(head)));
                }
                self.select_shapes(session, before, slide, &[shape], head)?;
                self.call(session, "smartArt.setDiagram", json!({"id": shape.0, "diagram": diagram}), head)
            }
            Payload::ChartSet { chart } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                // Address the chart before the first write: a command that names a shape which is
                // not a chart must not replace anything, whatever the value says.
                let is_chart =
                    before.slide(slide).and_then(|s| find_shape(&s.shapes, shape)).is_some_and(|sh| matches!(sh.kind, ShapeKind::Chart(_)));
                if !is_chart {
                    return Err(Rejection::invalid("the shape is not a chart", Some(head)));
                }
                let bytes = serde_json::to_vec(chart).map_err(|e| Rejection::invalid(format!("invalid chart: {e}"), Some(head)))?;
                if bytes.len() > MAX_CHART_JSON {
                    return Err(Rejection::invalid("the chart is too large", Some(head)));
                }
                self.select_shapes(session, before, slide, &[shape], head)?;
                self.call(session, "chart.setWhole", json!({"id": shape.0, "chart": chart}), head)
            }
            Payload::ShapeField { field, value } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                self.select_shapes(session, before, slide, &[shape], head)?;
                let name = match field {
                    ShapeField::Fill => "fill",
                    ShapeField::Line => "line",
                    ShapeField::Text => "text",
                    ShapeField::Table => "table",
                    ShapeField::SmartArt => "smartArt",
                    ShapeField::Chart => "chart",
                };
                self.call(session, "shape.setField", json!({"field": name, "value": value, "ids": [shape.0]}), head)
            }
            Payload::CellText { cell, body } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                // Address the cell before the first write: a body restored into a cell a row or
                // column edit removed would otherwise land somewhere nobody meant.
                let table = before
                    .slide(slide)
                    .and_then(|s| find_shape(&s.shapes, shape))
                    .ok_or_else(|| Rejection::stale("the shape is gone", Some(head)))?;
                let (r, c) = *cell;
                match &table.kind {
                    ShapeKind::Table(t) if t.cell(r as usize, c as usize).is_some() => {}
                    ShapeKind::Table(_) => {
                        return Err(Rejection::invalid(format!("the table has no cell ({r}, {c})"), Some(head)));
                    }
                    _ => return Err(Rejection::invalid("the shape is not a table", Some(head))),
                }
                self.select_shapes(session, before, slide, &[shape], head)?;
                self.call(session, "shape.setField", json!({"field": "cellText", "cell": [r, c], "value": body, "ids": [shape.0]}), head)
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
                            "field": match item.field { ShapeField::Fill => "fill", ShapeField::Line => "line", ShapeField::Text => "text", ShapeField::Table => "table", ShapeField::SmartArt => "smartArt", ShapeField::Chart => "chart" },
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

    /// Apply one `format.*` command over every body it addresses (RFC §3.2.1, §15).
    ///
    /// All addresses are resolved and checked before the first write, so a stale cell or an
    /// offset past the body refuses the whole operation instead of formatting part of it. The
    /// receiving user's selection is borrowed for the calls and put back afterwards.
    #[allow(clippy::too_many_arguments)]
    fn run_format(
        &mut self,
        session: &mut Session,
        before: &Presentation,
        head: u64,
        op: &OpEnvelope,
        format: &FormatCmd,
        args: &Value,
        addresses: &[FormatAddress],
    ) -> Result<(), Rejection> {
        let (slide, shape) = self.resolve_one(before, &op.target, head)?;
        let target = before.slide(slide).and_then(|s| find_shape(&s.shapes, shape));
        let sh = target.ok_or_else(|| Rejection::stale("the shape is gone", Some(head)))?;
        if !format.accepts(args) {
            return Err(Rejection::invalid(format!("`{}` does not take these arguments", format.id()), Some(head)));
        }
        // One operation over N bodies: the address shape is checked as a whole before
        // anything is written, so a list that mixes table cells and a shape body, or
        // names the shape body twice, cannot format half of what it meant.
        if addresses.is_empty() {
            return Err(Rejection::invalid("the format operation addresses no body", Some(head)));
        }
        if addresses.len() > MAX_FORMAT_ADDRESSES {
            return Err(Rejection::invalid("the format operation addresses too many bodies", Some(head)));
        }
        if matches!(&sh.kind, ShapeKind::Table(_)) {
            if addresses.iter().any(|address| address.cell.is_none()) {
                return Err(Rejection::unsupported("formatting a table needs a cell address", Some(head)));
            }
        } else {
            if addresses.iter().any(|address| address.cell.is_some()) {
                return Err(Rejection::invalid("the shape is not a table", Some(head)));
            }
            if addresses.len() != 1 {
                return Err(Rejection::invalid("a shape has one text body to format", Some(head)));
            }
            if sh.text.is_none() {
                return Err(Rejection::invalid("the shape has no text to format", Some(head)));
            }
        }
        // Every address is resolved against the authoritative body and checked before the
        // first write: a stale cell, an empty range or an offset past the end refuses the
        // whole step instead of formatting part of it.
        let mut seen: HashSet<(u32, u32)> = HashSet::new();
        let mut bodies: Vec<(Option<(u32, u32)>, Option<TextRange>, Option<u32>)> = Vec::with_capacity(addresses.len());
        for address in addresses {
            if address.range.is_some() && address.caret.is_some() {
                return Err(Rejection::invalid("a format address is a range or a caret, not both", Some(head)));
            }
            if let Some(cell) = address.cell
                && !seen.insert(cell)
            {
                return Err(Rejection::invalid(format!("the cell ({}, {}) is addressed twice", cell.0, cell.1), Some(head)));
            }
            let body = self
                .text_body(before, slide, shape, address.cell, head)?
                .ok_or_else(|| Rejection::invalid("the shape has no text to format", Some(head)))?;
            let inside = |offset: u32| {
                let at = offset_position(Some(body), offset as usize);
                position_offset(Some(body), at) == offset as usize
            };
            if let Some(range) = address.range {
                if range.to <= range.from {
                    return Err(Rejection::invalid("the formatted range is empty", Some(head)));
                }
                if !inside(range.from) || !inside(range.to) {
                    return Err(Rejection::invalid("the formatted range is outside the text body", Some(head)));
                }
            }
            if let Some(caret) = address.caret
                && !inside(caret)
            {
                return Err(Rejection::invalid("the formatted caret is outside the text body", Some(head)));
            }
            bodies.push((address.cell, address.range, address.caret));
        }
        // `format.*` acts on the engine's text selection: borrow it for the calls and put
        // the receiving user's selection back (formatting moves no text, so it stays
        // valid). The single-user command is the one that runs, so both paths agree —
        // and a caret address lets that command decide which characters it touches.
        let saved = session.doc().map_err(|_| Rejection::invalid("no document open", Some(head)))?.selection.clone();
        let mut out = Ok(());
        for (cell, range, caret) in bodies {
            let placed = if let Some((from, to)) = match (range, caret) {
                (Some(range), _) => Some((range.from, range.to)),
                (None, Some(caret)) => Some((caret, caret)),
                _ => None,
            } {
                let body = before.slide(slide).and_then(|s| find_shape(&s.shapes, shape)).and_then(|sh| match &sh.kind {
                    ShapeKind::Table(t) => cell.and_then(|(r, c)| t.cell(r as usize, c as usize)).map(|x| &x.text),
                    _ => sh.text.as_ref(),
                });
                let start = offset_position(body, from as usize);
                let end = offset_position(body, to as usize);
                let sel =
                    TextSel { shape, anchor: start, caret: end, cell: cell.map(|(r, c)| (r as usize, c as usize)), notes: false, line_end: None };
                self.select_text(session, before, slide, sel, head)
            } else {
                self.select_format(session, before, slide, shape, cell, head)
            };
            if let Err(rejection) = placed {
                if let Ok(st) = session.doc_mut() {
                    st.selection = saved.clone();
                }
                return Err(rejection);
            }
            out = self.call(session, format.id(), args.clone(), head);
            if out.is_err() {
                break;
            }
        }
        if let Ok(st) = session.doc_mut() {
            st.selection = saved;
        }
        out
    }

    /// Put back the whole bodies of N table cells in one atomic step (ADR 0010).
    ///
    /// Every cell and every body is validated before the first write, so a stale cell refuses
    /// the whole restore rather than leaving half the bodies in place.
    fn run_cells_text(
        &mut self,
        session: &mut Session,
        before: &Presentation,
        head: u64,
        op: &OpEnvelope,
        cells: &[CellBody],
    ) -> Result<(), Rejection> {
        let (slide, shape) = self.resolve_one(before, &op.target, head)?;
        let table =
            before.slide(slide).and_then(|s| find_shape(&s.shapes, shape)).ok_or_else(|| Rejection::stale("the shape is gone", Some(head)))?;
        let ShapeKind::Table(t) = &table.kind else {
            return Err(Rejection::invalid("the shape is not a table", Some(head)));
        };
        if cells.is_empty() {
            return Err(Rejection::invalid("the restore carries no cell bodies", Some(head)));
        }
        if cells.len() > MAX_CELL_BODIES {
            return Err(Rejection::invalid("the restore carries too many cell bodies", Some(head)));
        }
        let size: usize = cells.iter().map(|item| serde_json::to_vec(&item.body).map(|b| b.len()).unwrap_or(usize::MAX)).sum();
        if size > MAX_CELL_BODIES_JSON {
            return Err(Rejection::invalid("the restored cell bodies are too large", Some(head)));
        }
        // Every cell and every body is validated before the first write: the restore is
        // one step, so a stale cell must not leave half the bodies in place.
        let mut seen: HashSet<(u32, u32)> = HashSet::new();
        let mut bodies = Vec::with_capacity(cells.len());
        for item in cells {
            let (r, c) = item.cell;
            if !seen.insert(item.cell) {
                return Err(Rejection::invalid(format!("the cell ({r}, {c}) is restored twice"), Some(head)));
            }
            t.cell(r as usize, c as usize).ok_or_else(|| Rejection::invalid(format!("the table has no cell ({r}, {c})"), Some(head)))?;
            serde_json::from_value::<TextBody>(item.body.clone()).map_err(|e| Rejection::invalid(format!("invalid cell body: {e}"), Some(head)))?;
            bodies.push((item.cell, item.body.clone()));
        }
        self.select_shapes(session, before, slide, &[shape], head)?;
        for ((r, c), body) in bodies {
            self.call(session, "shape.setField", json!({"field": "cellText", "cell": [r, c], "value": body, "ids": [shape.0]}), head)?;
        }
        Ok(())
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

    /// Put the engine's selection on one shape — and, when the operation addresses a table cell,
    /// on that one cell. `format.*` acts on the selected cells of a table shape, so without the
    /// range it would run over every cell (or over the table's empty own body) instead of the
    /// cell this operation names.
    fn select_format(
        &self,
        session: &mut Session,
        before: &Presentation,
        slide: SlideId,
        shape: ShapeId,
        cell: Option<(u32, u32)>,
        head: u64,
    ) -> Result<(), Rejection> {
        let index = before.slide_index(slide).ok_or_else(|| Rejection::stale("the slide is gone", Some(head)))?;
        let st = session.doc_mut().map_err(|_| Rejection::invalid("no document open", Some(head)))?;
        st.selection.target = EngineTarget::Slides;
        st.selection.slide = index;
        st.selection.slides.clear();
        st.selection.shapes = vec![shape];
        st.selection.cells = cell.map(|(r, c)| (shape, (r as usize, c as usize), (r as usize, c as usize)));
        st.selection.text = None;
        Ok(())
    }

    /// The text body of `shape`, or of one table cell, that a text operation's offset is inside.
    /// A table without a cell address is refused: a cell has its own body, and the shape's own
    /// `text` is empty for a table.
    fn text_body<'a>(
        &self,
        before: &'a Presentation,
        slide: SlideId,
        shape: ShapeId,
        cell: Option<(u32, u32)>,
        head: u64,
    ) -> Result<Option<&'a TextBody>, Rejection> {
        let sh = before.slide(slide).and_then(|s| find_shape(&s.shapes, shape)).ok_or_else(|| Rejection::stale("the shape is gone", Some(head)))?;
        match (&sh.kind, cell) {
            (ShapeKind::Table(t), Some((r, c))) => t
                .cell(r as usize, c as usize)
                .map(|x| Some(&x.text))
                .ok_or_else(|| Rejection::invalid(format!("the table has no cell ({r}, {c})"), Some(head))),
            (ShapeKind::Table(_), None) => Err(Rejection::invalid("a table text operation needs a cell address", Some(head))),
            _ => Ok(sh.text.as_ref()),
        }
    }

    /// Put the engine's selection on one shape's text, so `text.insert`/`text.delete` act at the
    /// authoritative position rather than at whatever this replica had selected.
    fn select_text(&self, session: &mut Session, before: &Presentation, slide: SlideId, sel: TextSel, head: u64) -> Result<(), Rejection> {
        let index = before.slide_index(slide).ok_or_else(|| Rejection::stale("the slide is gone", Some(head)))?;
        let st = session.doc_mut().map_err(|_| Rejection::invalid("no document open", Some(head)))?;
        st.selection.target = EngineTarget::Slides;
        st.selection.slide = index;
        st.selection.slides.clear();
        st.selection.shapes = vec![sel.shape];
        st.selection.cells = None;
        st.selection.text = Some(sel);
        Ok(())
    }

    /// Run one notes command without moving the receiving user's caret. `text.insert` and
    /// `text.delete` act on the engine selection, so a notes edit borrows it for the call and puts
    /// it back afterwards, shifted by the characters the edit added or removed before it on the
    /// same slide (RFC §4.4). The command itself is the single-user one, so both paths produce the
    /// same document.
    #[allow(clippy::too_many_arguments)]
    fn call_notes(
        &self,
        session: &mut Session,
        before: &Presentation,
        slide: SlideId,
        sel: Option<TextSel>,
        shift: NotesShift,
        cmd: &str,
        args: Value,
        head: u64,
    ) -> Result<(), Rejection> {
        let index = before.slide_index(slide).ok_or_else(|| Rejection::stale("the slide is gone", Some(head)))?;
        let saved = session.doc().map_err(|_| Rejection::invalid("no document open", Some(head)))?.selection.clone();
        let notes_before = before.slide(slide).map(|s| s.notes.clone()).unwrap_or_default();
        if let Some(sel) = sel {
            let st = session.doc_mut().map_err(|_| Rejection::invalid("no document open", Some(head)))?;
            st.selection.target = EngineTarget::Slides;
            st.selection.slide = index;
            st.selection.slides.clear();
            st.selection.shapes.clear();
            st.selection.cells = None;
            st.selection.text = Some(sel);
        }
        let out = self.call(session, cmd, args, head);
        // Put the receiving user's selection back: a remote notes edit must not move their caret.
        if let Ok(st) = session.doc_mut() {
            let notes_after = st.doc.slides.get(index).map(|s| s.notes.clone()).unwrap_or_default();
            st.selection = restore_notes_selection(saved, index, &notes_before, &notes_after, &shift);
        }
        out
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

    /// Answer one **read-only** engine query against the authoritative document. The address is
    /// resolved from session keys, the query runs on this session's engine, and the engine's
    /// selection is put back afterwards, so a query changes neither the document nor what a later
    /// operation or undo sees. A command that changes the document is refused: it belongs on the
    /// operation path, where it gets a version and an undo image. The engine does the rest — an
    /// unknown command, an unaddressable object and a feature that is not a query all fail there.
    pub fn query(&self, session: &mut Session, cmd: &str, target: &Target, params: &Value, head: u64) -> Result<Value, Rejection> {
        if !QUERY_COMMANDS.contains(&cmd) {
            return Err(Rejection::unsupported(format!("`{cmd}` is not a query this server answers"), Some(head)));
        }
        let spec = deckcraft_engine::cmd::find_command(cmd)
            .ok_or_else(|| Rejection::unsupported(format!("`{cmd}` is not a command this engine knows"), Some(head)))?;
        if spec.changes_document() {
            return Err(Rejection::invalid(format!("`{cmd}` changes the document; send it as an operation"), Some(head)));
        }
        let (before, saved) = {
            let st = session.doc().map_err(|_| Rejection::invalid("no document open", Some(head)))?;
            (st.doc.clone(), st.selection.clone())
        };
        let mut address = None;
        if target.slide.is_some() || target.shape.is_some() || !target.shapes.is_empty() {
            let slide = self.resolve_slide(&before, target.slide.as_ref(), head)?;
            let ids = if target.shapes.is_empty() {
                match target.shape.as_ref() {
                    Some(key) => vec![self.resolve_shape(&before, slide, key, head)?],
                    None => Vec::new(),
                }
            } else {
                let mut ids = Vec::with_capacity(target.shapes.len());
                for key in &target.shapes {
                    ids.push(self.resolve_shape(&before, slide, key, head)?);
                }
                ids
            };
            self.select_shapes(session, &before, slide, &ids, head)?;
            address = ids.first().copied();
        }
        let mut args = match params {
            Value::Object(_) => params.clone(),
            _ => json!({}),
        };
        if let Some(id) = address {
            args["id"] = json!(id.0);
        }
        let result = session.execute(cmd, &args);
        if let Ok(st) = session.doc_mut() {
            st.selection = saved;
        }
        result.map_err(|e| Rejection::invalid(format!("{cmd}: {e}"), Some(head)))
    }

    /// The table a table command addresses, resolved from its slide and shape keys before the
    /// first write: an unknown key, a shape of another slide, a shape that is not a table and a
    /// tombstone are refused here, so no table command can act on something else.
    fn table_of<'a>(&self, doc: &'a Presentation, slide: SlideId, shape: ShapeId, head: u64) -> Result<&'a Table, Rejection> {
        let target = doc.slide(slide).and_then(|s| find_shape(&s.shapes, shape)).ok_or_else(|| Rejection::stale("the shape is gone", Some(head)))?;
        match &target.kind {
            ShapeKind::Table(table) => Ok(table),
            _ => Err(Rejection::invalid("the shape is not a table", Some(head))),
        }
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
        ShapeField::Table => Image::Table(deckcraft_model::Table::default()),
        ShapeField::SmartArt => Image::SmartArt(Value::Null),
        ShapeField::Chart => Image::Chart(Value::Null),
    }
}

/// The SmartArt group a shape is, or the graphic that contains it.
fn smartart_group(doc: &Presentation, slide: SlideId, shape: ShapeId) -> Option<&Shape> {
    let shapes = &doc.slide(slide)?.shapes;
    let mut id = Some(shape);
    for _ in 0..64 {
        let sh = find_shape(shapes, id?)?;
        if sh.diagram.is_some() {
            return Some(sh);
        }
        // `None` = a top-level shape (nothing above to look at); `Some(None)` = the same.
        id = match parent_of(shapes, sh.id) {
            Some(Some(group)) => Some(group),
            _ => return None,
        };
    }
    None
}

/// Longest diagram value a `smartArt.setDiagram` or an insertion may carry, as JSON bytes. The
/// engine applies the same bound.
const MAX_SMARTART_JSON: usize = deckcraft_engine::cmd::smartart::MAX_DIAGRAM_JSON;
/// Longest chart value a `chart.setWhole` or a `chart_insert` may carry, as JSON bytes (a kept
/// chart part travels inside the value; the engine applies the same bound).
const MAX_CHART_JSON: usize = deckcraft_engine::cmd::insert::MAX_CHART_JSON;

/// The read-only queries the server answers on the wire. A query gets no version and no journal
/// entry, so this is deliberately an allowlist: an arbitrary engine query (one that renders,
/// lists the server's files or reads its dictionaries) must never be reachable this way.
const QUERY_COMMANDS: [&str; 1] = ["smartArt.nodes"];

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

/// How one notes edit moves the text before a caret inside the same notes body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NotesShift {
    Insert {
        at: u32,
        len: u32,
    },
    Delete {
        from: u32,
        to: u32,
    },
    /// A whole-body replacement: the caret keeps its offset, clamped to the new body.
    Replace,
}

/// Put the receiving user's selection back after a remote notes edit. A caret that was inside the
/// same notes body shifts by the characters the edit added or removed before it; a caret in a
/// shape's text, on another slide, or outside the notes stays exactly where it was.
fn restore_notes_selection(mut saved: Selection, index: usize, before_body: &TextBody, after_body: &TextBody, shift: &NotesShift) -> Selection {
    if saved.slide == index
        && let Some(sel) = saved.text.as_mut()
        && sel.notes
    {
        let moved = |pos: (usize, usize)| {
            let offset = position_offset(Some(before_body), pos) as u32;
            offset_position(Some(after_body), shift_notes_offset(offset, shift) as usize)
        };
        sel.anchor = moved(sel.anchor);
        sel.caret = moved(sel.caret);
        sel.line_end = None;
    }
    saved
}

/// A whole-body notes offset after an edit: an insertion at or before it pushes it right, a
/// deletion before it pulls it left, and a range that swallows it clamps to the range's start.
fn shift_notes_offset(point: u32, shift: &NotesShift) -> u32 {
    match *shift {
        NotesShift::Insert { at, len } => {
            if at <= point {
                point.saturating_add(len)
            } else {
                point
            }
        }
        NotesShift::Delete { from, to } => {
            if to <= point {
                point.saturating_sub(to.saturating_sub(from))
            } else if from < point {
                from
            } else {
                point
            }
        }
        NotesShift::Replace => point,
    }
}

/// One embedded font entry as the history stores it: present with its whole model value, or
/// absent. A document font is presentation-level, so no slide is resolved.
fn capture_font(doc: &Presentation, typeface: &str) -> FontImage {
    FontImage { typeface: typeface.to_string(), font: doc.embedded_fonts.iter().find(|f| f.typeface.eq_ignore_ascii_case(typeface)).cloned() }
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
        | Payload::SlideTransition { .. }
        | Payload::ShapeAnimations { .. }
        | Payload::SlideAnimations { .. }
        | Payload::NotesSet { .. }
        | Payload::NotesInsert { .. }
        | Payload::NotesDelete { .. }
        | Payload::ShapeField { .. }
        | Payload::CellText { .. }
        | Payload::CellsText { .. }
        | Payload::ShapeFields { .. }
        | Payload::FontCreate { .. }
        | Payload::FontSet { .. }
        | Payload::FontRemove { .. }
        | Payload::FontRestore { .. }
        | Payload::TableStyle { .. }
        | Payload::TableCellFill { .. }
        | Payload::TableMerge { .. }
        | Payload::TableSplit { .. }
        | Payload::TableColumnWidth { .. }
        | Payload::TableRowHeight { .. }
        | Payload::TableOptions { .. }
        | Payload::SmartArtSet { .. }
        | Payload::ChartSet { .. } => OpKind::Command,
        Payload::ShapeInsert { .. }
        | Payload::PictureInsert { .. }
        | Payload::MediaInsert { .. }
        | Payload::ChartInsert { .. }
        | Payload::SmartArtInsert { .. }
        | Payload::ShapeDelete {}
        | Payload::ShapeRestore { .. }
        | Payload::Reorder { .. }
        | Payload::Group {}
        | Payload::Ungroup {}
        | Payload::SlideInsert { .. }
        | Payload::SlideDelete {}
        | Payload::SlideRestore { .. }
        | Payload::SlideMove { .. }
        | Payload::SlideDuplicate {}
        | Payload::TableEdit { .. } => OpKind::Structural,
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
    use deckcraft_collab_proto::{
        CellFillSpec, FillSpec, FormatCmd, LineSpec, ShapeBounds, ShapeField, ShapeRestoreItem, TextCursor, TransitionSpec,
    };

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

    /// The same deck with a 2×2 table as the third shape, and its top-left cell seeded.
    fn table_deck() -> Session {
        let mut s = deck();
        s.apply_remote("insert.table", &json!({"rows": 2, "cols": 2, "rect": [100, 300, 300, 100]})).expect("table");
        let table = s.doc().expect("doc").doc.slides[0].shapes[2].id;
        s.apply_remote("text.set", &json!({"id": table.0, "cell": [0, 0], "text": "seed"})).expect("seed");
        s
    }

    /// The same deck with a chart as the third shape.
    fn chart_deck() -> Session {
        let mut s = deck();
        s.apply_remote("insert.chart", &json!({"type": "column", "categories": ["Q1", "Q2"], "series": [{"name": "Sales", "values": [4.0, 5.0]}]}))
            .expect("chart");
        s
    }

    /// The same deck with a SmartArt graphic as the third shape (Plan, Scope under it, Build).
    fn smartart_deck() -> Session {
        let mut s = deck();
        s.apply_remote("insert.smartArt", &json!({"kind": "blockList", "items": ["Plan", "\tScope", "Build"]})).expect("smartart");
        s
    }

    fn cell_text(doc: &Presentation, shape: usize, r: usize, c: usize) -> String {
        match &doc.slides[0].shapes[shape].kind {
            ShapeKind::Table(t) => t.cell(r, c).expect("cell").text.text(),
            other => panic!("shape {shape} is not a table: {other:?}"),
        }
    }

    fn table_rows(doc: &Presentation, shape: usize) -> usize {
        match &doc.slides[0].shapes[shape].kind {
            ShapeKind::Table(t) => t.n_rows(),
            other => panic!("shape {shape} is not a table: {other:?}"),
        }
    }

    /// One cell's whole body, as the exact image an undo of a cell step carries.
    fn cell_body(doc: &Presentation, shape: usize, r: usize, c: usize) -> deckcraft_model::text::TextBody {
        match &doc.slides[0].shapes[shape].kind {
            ShapeKind::Table(t) => t.cell(r, c).expect("cell").text.clone(),
            other => panic!("shape {shape} is not a table: {other:?}"),
        }
    }

    /// The bold flag of the first run of one cell (`None` when the cell has no runs).
    fn cell_bold(doc: &Presentation, shape: usize, r: usize, c: usize) -> Option<bool> {
        cell_body(doc, shape, r, c).paragraphs.iter().flat_map(|p| p.runs.iter()).find_map(|run| run.props.bold)
    }

    /// `(run text, bold)` for every run of one cell's first paragraph.
    fn cell_bolds(doc: &Presentation, shape: usize, r: usize, c: usize) -> Vec<(String, Option<bool>)> {
        cell_body(doc, shape, r, c).paragraphs.iter().flat_map(|p| p.runs.iter()).map(|run| (run.text.clone(), run.props.bold)).collect()
    }

    /// The italic flag of the first run of one cell (`None` when the cell has no runs).
    fn cell_italic(doc: &Presentation, shape: usize, r: usize, c: usize) -> Option<bool> {
        cell_body(doc, shape, r, c).paragraphs.iter().flat_map(|p| p.runs.iter()).find_map(|run| run.props.italic)
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
    fn two_clients_type_in_different_cells_of_one_table() {
        let (mut a, mut b) = (table_deck(), table_deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let doc = doc_of(&a);
        let (slide, shape) = key_of(&aa, &doc, 2);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };

        let first = op(OpKind::Command, 0, target.clone(), Payload::TextInsert { at: 0, text: "A".into(), cell: Some((0, 0)) });
        let mut second = op(OpKind::Command, 1, target, Payload::TextInsert { at: 0, text: "B".into(), cell: Some((1, 1)) });
        second.op_id = "op-2".into();
        aa.apply(&mut a, 0, "a", &first).expect("first");
        bb.apply(&mut b, 0, "a", &first).expect("first on b");
        aa.apply(&mut a, 1, "b", &second).expect("second");
        bb.apply(&mut b, 1, "b", &second).expect("second on b");

        assert_eq!(doc_of(&a), doc_of(&b), "the replicas converge");
        assert_eq!(cell_text(&doc_of(&a), 2, 0, 0), "Aseed");
        assert_eq!(cell_text(&doc_of(&a), 2, 1, 1), "B");
    }

    #[test]
    fn undo_of_cell_text_steps_restores_the_exact_cell_value() {
        let mut s = table_deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let doc = doc_of(&s);
        let (slide, shape) = key_of(&ad, &doc, 2);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };

        let insert = op(OpKind::Command, 0, target.clone(), Payload::TextInsert { at: 4, text: "Z".into(), cell: Some((0, 0)) });
        ad.apply(&mut s, 0, "a", &insert).expect("insert");
        assert_eq!(cell_text(&doc_of(&s), 2, 0, 0), "seedZ");
        assert!(doc_of(&s).slides[0].shapes[2].text.is_none(), "a cell edit does not create shape text");

        let undo = ad.undo(&s, 1, "a", "u-1").expect("undo");
        assert!(matches!(undo.payload, Payload::TextSet { cell: Some((0, 0)), ref text } if text == "seed"), "{undo:?}");
        ad.apply(&mut s, 1, "a", &undo).expect("apply undo");
        assert_eq!(cell_text(&doc_of(&s), 2, 0, 0), "seed");

        let redo = ad.redo(&s, 2, "a", "r-1").expect("redo");
        ad.apply(&mut s, 2, "a", &redo).expect("apply redo");
        assert_eq!(cell_text(&doc_of(&s), 2, 0, 0), "seedZ");

        let mut delete = op(OpKind::Command, 3, target, Payload::TextDelete { from: 0, to: 4, cell: Some((0, 0)) });
        delete.op_id = "op-del".into();
        ad.apply(&mut s, 3, "a", &delete).expect("delete");
        assert_eq!(cell_text(&doc_of(&s), 2, 0, 0), "Z");
        let undo = ad.undo(&s, 4, "a", "u-2").expect("undo delete");
        assert!(matches!(undo.payload, Payload::TextSet { cell: Some((0, 0)), ref text } if text == "seedZ"), "{undo:?}");
        ad.apply(&mut s, 4, "a", &undo).expect("apply undo");
        assert_eq!(cell_text(&doc_of(&s), 2, 0, 0), "seedZ");
    }

    #[test]
    fn two_clients_converge_on_formatting_one_table_cell() {
        let (mut a, mut b) = (table_deck(), table_deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let doc = doc_of(&a);
        let (slide, shape) = key_of(&aa, &doc, 2);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };
        let bold = op(OpKind::Command, 0, target, format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(Some((0, 0)), None, None)]));

        let da = aa.apply(&mut a, 0, "a", &bold).expect("apply on a");
        let db = bb.apply(&mut b, 0, "b", &bold).expect("apply on b");
        assert_eq!(da, db, "the same operation has the same delta on both replicas");
        assert_eq!(doc_of(&a), doc_of(&b), "the replicas converge");
        assert_eq!(cell_bold(&doc_of(&a), 2, 0, 0), Some(true), "the addressed cell is bold");
        assert_eq!(cell_text(&doc_of(&a), 2, 0, 0), "seed", "formatting keeps the cell text");
        assert_eq!(cell_bold(&doc_of(&a), 2, 0, 1), None, "a neighbouring cell keeps its formatting");
        assert_eq!(cell_bold(&doc_of(&a), 2, 1, 1), None);
        assert_eq!(cell_text(&doc_of(&a), 2, 0, 1), "", "and its text");
        assert!(doc_of(&a).slides[0].shapes[2].text.is_none(), "a cell format does not create shape text");
    }

    #[test]
    fn undo_of_a_cell_format_restores_the_old_body_and_leaves_neighbours_alone() {
        let mut s = table_deck();
        // A pre-existing formatting on the cell and a seeded neighbour: the undo must put back
        // the whole body (italic kept, bold gone) and must not touch the other cell.
        let table = doc_of(&s).slides[0].shapes[2].id;
        s.doc_mut().expect("doc").selection.shapes = vec![table];
        s.doc_mut().expect("doc").selection.cells = Some((table, (0, 0), (0, 0)));
        s.apply_remote("format.italic", &json!({})).expect("seed italic");
        s.apply_remote("text.set", &json!({"id": table.0, "cell": [1, 1], "text": "neighbour"})).expect("seed the neighbour");
        let was = cell_body(&doc_of(&s), 2, 0, 0);
        assert_eq!(cell_italic(&doc_of(&s), 2, 0, 0), Some(true), "the seed formatting is in place");

        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let doc = doc_of(&s);
        let (slide, shape) = key_of(&ad, &doc, 2);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };

        let bold = op(OpKind::Command, 0, target.clone(), format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(Some((0, 0)), None, None)]));
        ad.apply(&mut s, 0, "a", &bold).expect("a bolds the cell");
        assert_eq!(cell_bold(&doc_of(&s), 2, 0, 0), Some(true));

        let mut italic = op(OpKind::Command, 1, target, format_set(FormatCmd::Italic, json!({"on": true}), vec![addr(Some((1, 1)), None, None)]));
        italic.op_id = "op-2".into();
        ad.apply(&mut s, 1, "b", &italic).expect("b italicises the neighbour");
        assert_eq!(cell_italic(&doc_of(&s), 2, 1, 1), Some(true));

        let undo = ad.undo(&s, 2, "a", "u-1").expect("undo a's cell format");
        assert!(matches!(undo.payload, Payload::CellText { cell: (0, 0), .. }), "the restore names the cell: {undo:?}");
        ad.apply(&mut s, 2, "a", &undo).expect("apply the undo");
        assert_eq!(cell_body(&doc_of(&s), 2, 0, 0), was, "the whole body of that cell came back");
        assert_eq!(cell_bold(&doc_of(&s), 2, 0, 0), None, "the bold is gone");
        assert_eq!(cell_italic(&doc_of(&s), 2, 0, 0), Some(true), "the formatting that was there is back");
        assert_eq!(cell_italic(&doc_of(&s), 2, 1, 1), Some(true), "the neighbour's step is untouched");
        assert_eq!(cell_text(&doc_of(&s), 2, 1, 1), "neighbour");

        let redo = ad.redo(&s, 3, "a", "r-1").expect("redo a's cell format");
        ad.apply(&mut s, 3, "a", &redo).expect("apply the redo");
        assert_eq!(cell_bold(&doc_of(&s), 2, 0, 0), Some(true), "the redo puts the bold back");
        assert_eq!(cell_italic(&doc_of(&s), 2, 1, 1), Some(true), "and still leaves the neighbour alone");
    }

    #[test]
    fn an_undo_of_a_cell_format_is_refused_when_someone_else_changed_that_cell() {
        let mut s = table_deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let doc = doc_of(&s);
        let (slide, shape) = key_of(&ad, &doc, 2);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };

        let bold = op(OpKind::Command, 0, target.clone(), format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(Some((0, 0)), None, None)]));
        ad.apply(&mut s, 0, "a", &bold).expect("a bolds");
        let mut insert = op(OpKind::Command, 1, target, Payload::TextInsert { at: 4, text: "X".into(), cell: Some((0, 0)) });
        insert.op_id = "op-2".into();
        ad.apply(&mut s, 1, "b", &insert).expect("b types in the same cell");
        assert_eq!(cell_text(&doc_of(&s), 2, 0, 0), "seedX");

        let err = ad.undo(&s, 2, "a", "u-1").expect_err("b changed the same cell body");
        assert_eq!(err.code, RejectCode::UndoConflict);
        assert!(!err.retryable, "an undo never overwrites someone else's edit");
        assert_eq!(ad.history().get("a").map(|h| h.undo_len()), Some(1), "a refused undo moves nothing");
        assert_eq!(cell_text(&doc_of(&s), 2, 0, 0), "seedX", "and writes nothing");
    }

    #[test]
    fn two_clients_converge_on_formatting_a_range_of_cells_with_a_neighbour_untouched() {
        let (mut a, mut b) = (table_deck(), table_deck());
        let table = doc_of(&a).slides[0].shapes[2].id;
        // A second addressed cell and a neighbour the address does not name.
        for s in [&mut a, &mut b] {
            s.apply_remote("text.set", &json!({"id": table.0, "cell": [0, 1], "text": "wide"})).expect("seed the second cell");
            s.apply_remote("text.set", &json!({"id": table.0, "cell": [1, 0], "text": "keep"})).expect("seed the neighbour");
        }
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let doc = doc_of(&a);
        let (slide, shape) = key_of(&aa, &doc, 2);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };
        // One operation, two cells: the first whole, the second over two characters.
        let many = op(
            OpKind::Command,
            0,
            target,
            format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(Some((0, 0)), None, None), addr(Some((0, 1)), range(0, 2), None)]),
        );

        let da = aa.apply(&mut a, 0, "a", &many).expect("apply on a");
        let db = bb.apply(&mut b, 0, "b", &many).expect("apply on b");
        assert_eq!(da, db, "the same operation has the same delta on both replicas");
        assert_eq!(doc_of(&a), doc_of(&b), "the replicas converge");
        assert_eq!(cell_text(&doc_of(&a), 2, 0, 0), "seed", "formatting keeps the cell text");
        assert!(cell_body(&doc_of(&a), 2, 0, 0).paragraphs.iter().flat_map(|p| p.runs.iter()).all(|r| r.props.bold == Some(true)));
        assert_eq!(
            cell_bolds(&doc_of(&a), 2, 0, 1),
            vec![("wi".to_string(), Some(true)), ("de".to_string(), None)],
            "only the addressed range of the second cell is bold"
        );
        assert_eq!(cell_text(&doc_of(&a), 2, 0, 1), "wide");
        assert_eq!(cell_bolds(&doc_of(&a), 2, 1, 0), vec![("keep".to_string(), None)], "the neighbour is untouched");
        assert_eq!(cell_bolds(&doc_of(&a), 2, 1, 1), Vec::<(String, Option<bool>)>::new());
    }

    #[test]
    fn undo_of_a_multi_cell_format_restores_exactly_those_cells_and_redo_reapplies_it() {
        let mut s = table_deck();
        let table = doc_of(&s).slides[0].shapes[2].id;
        s.apply_remote("text.set", &json!({"id": table.0, "cell": [0, 1], "text": "wide"})).expect("seed the second cell");
        s.apply_remote("text.set", &json!({"id": table.0, "cell": [1, 1], "text": "neighbour"})).expect("seed the neighbour");
        // Pre-existing formatting on both addressed cells, and a neighbour with its own.
        for (r, c) in [(0, 0), (0, 1), (1, 1)] {
            s.doc_mut().expect("doc").selection.shapes = vec![table];
            s.doc_mut().expect("doc").selection.cells = Some((table, (r, c), (r, c)));
            s.apply_remote("format.italic", &json!({})).expect("seed italic");
        }
        let was_first = cell_body(&doc_of(&s), 2, 0, 0);
        let was_second = cell_body(&doc_of(&s), 2, 0, 1);
        let was_neighbour = cell_body(&doc_of(&s), 2, 1, 1);

        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let doc = doc_of(&s);
        let (slide, shape) = key_of(&ad, &doc, 2);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };

        let bold = op(
            OpKind::Command,
            0,
            target,
            format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(Some((0, 0)), None, None), addr(Some((0, 1)), None, None)]),
        );
        ad.apply(&mut s, 0, "a", &bold).expect("a bolds both cells");
        assert_eq!(cell_bold(&doc_of(&s), 2, 0, 0), Some(true));
        assert_eq!(cell_bold(&doc_of(&s), 2, 0, 1), Some(true));

        let undo = ad.undo(&s, 1, "a", "u-1").expect("undo the multi-cell format");
        match &undo.payload {
            Payload::CellsText { cells } => {
                let mut named: Vec<(u32, u32)> = cells.iter().map(|item| item.cell).collect();
                named.sort();
                assert_eq!(named, vec![(0, 0), (0, 1)], "the restore names exactly the addressed cells");
            }
            other => panic!("the multi-cell restore must be one setCellBodies: {other:?}"),
        }
        ad.apply(&mut s, 1, "a", &undo).expect("apply the undo");
        assert_eq!(cell_body(&doc_of(&s), 2, 0, 0), was_first, "the first cell's whole body came back");
        assert_eq!(cell_body(&doc_of(&s), 2, 0, 1), was_second, "and the second's");
        assert_eq!(cell_body(&doc_of(&s), 2, 1, 1), was_neighbour, "the neighbour's body never moved");

        let redo = ad.redo(&s, 2, "a", "r-1").expect("redo the multi-cell format");
        ad.apply(&mut s, 2, "a", &redo).expect("apply the redo");
        assert_eq!(cell_bold(&doc_of(&s), 2, 0, 0), Some(true), "the redo bolds both cells again");
        assert_eq!(cell_bold(&doc_of(&s), 2, 0, 1), Some(true));
        assert_eq!(cell_italic(&doc_of(&s), 2, 1, 1), Some(true), "and still leaves the neighbour alone");
        assert_eq!(cell_text(&doc_of(&s), 2, 1, 1), "neighbour");
    }

    #[test]
    fn an_undo_of_a_multi_cell_format_is_refused_when_someone_changed_one_addressed_cell() {
        let mut s = table_deck();
        let table = doc_of(&s).slides[0].shapes[2].id;
        s.apply_remote("text.set", &json!({"id": table.0, "cell": [0, 1], "text": "wide"})).expect("seed the second cell");
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let doc = doc_of(&s);
        let (slide, shape) = key_of(&ad, &doc, 2);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };

        let bold = op(
            OpKind::Command,
            0,
            target.clone(),
            format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(Some((0, 0)), None, None), addr(Some((0, 1)), None, None)]),
        );
        ad.apply(&mut s, 0, "a", &bold).expect("a bolds both cells");
        let before = doc_of(&s);
        // A foreign edit of the second addressed cell: the undo may not overwrite it.
        let mut insert = op(OpKind::Command, 1, target, Payload::TextInsert { at: 4, text: "X".into(), cell: Some((0, 1)) });
        insert.op_id = "op-2".into();
        ad.apply(&mut s, 1, "b", &insert).expect("b types in the second cell");

        let err = ad.undo(&s, 2, "a", "u-1").expect_err("one addressed cell was changed");
        assert_eq!(err.code, RejectCode::UndoConflict);
        assert!(!err.retryable);
        assert_eq!(ad.history().get("a").map(|h| h.undo_len()), Some(1), "a refused undo moves nothing");
        assert_eq!(cell_text(&doc_of(&s), 2, 0, 1), "wideX", "and writes nothing");
        assert_eq!(cell_text(&doc_of(&s), 2, 0, 0), "seed");
        assert_ne!(doc_of(&s), before, "the foreign edit is what changed the document");
    }

    #[test]
    fn a_remote_multi_cell_format_leaves_the_receiving_selection_alone() {
        let (mut a, mut b) = (table_deck(), table_deck());
        let table = doc_of(&a).slides[0].shapes[2].id;
        for s in [&mut a, &mut b] {
            s.apply_remote("text.set", &json!({"id": table.0, "cell": [0, 1], "text": "wide"})).expect("seed the second cell");
        }
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let doc = doc_of(&a);
        let (slide, shape) = key_of(&aa, &doc, 2);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };
        // b is looking somewhere else: its own selection must survive the remote step.
        let other = doc_of(&b).slides[0].shapes[0].id;
        b.doc_mut().expect("doc").selection.shapes = vec![other];
        b.doc_mut().expect("doc").selection.text =
            Some(TextSel { shape: other, anchor: (0, 1), caret: (0, 2), cell: None, notes: false, line_end: None });
        let saved = b.doc().expect("doc").selection.clone();

        let many = op(
            OpKind::Command,
            0,
            target,
            format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(Some((0, 0)), None, None), addr(Some((0, 1)), None, None)]),
        );
        aa.apply(&mut a, 0, "a", &many).expect("apply on a");
        bb.apply(&mut b, 0, "b", &many).expect("apply on b");
        assert_eq!(b.doc().expect("doc").selection, saved, "the receiving user's selection is where they left it");
        assert_eq!(cell_bold(&doc_of(&b), 2, 0, 1), Some(true));
    }

    #[test]
    fn a_caret_in_a_shape_body_is_its_own_address_for_the_engine_command() {
        let (mut a, mut b) = (text_deck(), text_deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let doc = doc_of(&a);
        let (slide, shape) = key_of(&aa, &doc, 0);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };
        // "Hello world", caret inside "world": the single-user command widens it to the word,
        // and every replica runs that same command on the same authoritative body.
        let caret = op(OpKind::Command, 0, target, format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(None, None, Some(8))]));
        let da = aa.apply(&mut a, 0, "a", &caret).expect("apply on a");
        let db = bb.apply(&mut b, 0, "b", &caret).expect("apply on b");
        assert_eq!(da, db);
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(run_bolds(&doc_of(&a), 0), vec![("Hello ".into(), None), ("world".into(), Some(true))]);
        assert_eq!(doc_of(&a).slides[0].shapes[0].text.as_ref().expect("text").text(), "Hello world");

        // Undo puts the whole body back, and redo applies it again.
        let undo = aa.undo(&a, 1, "a", "u-1").expect("undo the caret format");
        assert!(matches!(undo.payload, Payload::ShapeField { field: ShapeField::Text, .. }), "a shape body is the image: {undo:?}");
        aa.apply(&mut a, 1, "a", &undo).expect("apply the undo");
        assert_eq!(run_bolds(&doc_of(&a), 0), vec![("Hello world".into(), None)]);
        let redo = aa.redo(&a, 2, "a", "r-1").expect("redo the caret format");
        aa.apply(&mut a, 2, "a", &redo).expect("apply the redo");
        assert_eq!(run_bolds(&doc_of(&a), 0), vec![("Hello ".into(), None), ("world".into(), Some(true))], "the redo puts the word back");
    }

    #[test]
    fn a_caret_at_a_word_boundary_is_not_widened_and_a_paragraph_command_takes_the_paragraph() {
        // A caret is not pre-widened by the replica: at a word boundary the engine sets the
        // formatting for what is typed next and leaves the existing runs alone.
        let mut s = text_deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let doc = doc_of(&s);
        let (slide, shape) = key_of(&ad, &doc, 0);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };
        let boundary = op(OpKind::Command, 0, target.clone(), format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(None, None, Some(5))]));
        ad.apply(&mut s, 0, "a", &boundary).expect("bold at the space");
        assert_eq!(run_bolds(&doc_of(&s), 0), vec![("Hello world".into(), None)], "no existing character became bold");

        // A paragraph command at a caret takes the paragraph it is in, not a word.
        let mut two = text_deck();
        let id = doc_of(&two).slides[0].shapes[0].id;
        two.apply_remote("text.set", &json!({"id": id.0, "text": "Hello\nWorld"})).expect("two paragraphs");
        let mut ad2 = Adapter::new("k");
        ad2.join(&doc_of(&two));
        let doc = doc_of(&two);
        let (slide, shape) = key_of(&ad2, &doc, 0);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };
        let centred = op(OpKind::Command, 0, target, format_set(FormatCmd::AlignCenter, json!({}), vec![addr(None, None, Some(7))]));
        ad2.apply(&mut two, 0, "a", &centred).expect("centre the second paragraph");
        let aligns: Vec<Option<deckcraft_model::text::Align>> =
            doc_of(&two).slides[0].shapes[0].text.as_ref().expect("text").paragraphs.iter().map(|p| p.props.align).collect();
        assert_eq!(aligns, vec![None, Some(deckcraft_model::text::Align::Center)], "only the paragraph under the caret");
        assert_eq!(doc_of(&two).slides[0].shapes[0].text.as_ref().expect("text").text(), "Hello\nWorld");
    }

    #[test]
    fn a_caret_outside_the_shape_body_is_refused_before_any_write() {
        let mut s = text_deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let before = doc_of(&s);
        let doc = doc_of(&s);
        let (slide, shape) = key_of(&ad, &doc, 0);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };
        let past = op(OpKind::Command, 0, target, format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(None, None, Some(99))]));
        let err = ad.apply(&mut s, 0, "a", &past).expect_err("the caret is past the body");
        assert_eq!(err.code, RejectCode::Invalid);
        assert_eq!(doc_of(&s), before, "a refused caret writes nothing");
        assert_eq!(ad.history().get("a").map(|h| h.undo_len()).unwrap_or(0), 0, "and records no step");
    }

    #[test]
    fn formatting_a_cell_that_is_not_there_is_refused_before_any_write() {
        let mut s = table_deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let doc = doc_of(&s);
        let (slide, shape) = key_of(&ad, &doc, 2);
        let before = doc_of(&s);
        let target = Target { slide: Some(slide.clone()), shape: Some(shape), ..Target::default() };

        let missing = op(OpKind::Command, 0, target.clone(), format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(Some((5, 5)), None, None)]));
        let err = ad.apply(&mut s, 0, "a", &missing).expect_err("the table has no cell (5, 5)");
        assert_eq!(err.code, RejectCode::Invalid);
        assert!(err.message.contains("no cell"), "{}", err.message);
        assert_eq!(doc_of(&s), before, "a refused address writes nothing");
        assert_eq!(ad.history().get("a").map(|h| h.undo_len()).unwrap_or(0), 0, "and records no step");

        // A table without a cell address has no body of its own to format.
        let whole = op(OpKind::Command, 0, target.clone(), format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(None, None, None)]));
        let err = ad.apply(&mut s, 0, "a", &whole).expect_err("a table needs a cell address");
        assert_eq!(err.code, RejectCode::Unsupported);
        assert_eq!(doc_of(&s), before);

        // A shape that is not a table takes no cell address.
        let other = ad.keys().shape_key(doc.slides[0].id, doc.slides[0].shapes[0].id).expect("shape key").clone();
        let wrong = op(
            OpKind::Command,
            0,
            Target { slide: Some(slide), shape: Some(other), ..Target::default() },
            format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(Some((0, 0)), None, None)]),
        );
        let err = ad.apply(&mut s, 0, "a", &wrong).expect_err("the shape is not a table");
        assert_eq!(err.code, RejectCode::Invalid);
        assert_eq!(doc_of(&s), before);
    }

    #[test]
    fn formatting_a_cell_on_a_deleted_slide_is_refused_before_any_write() {
        let mut s = table_deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let doc = doc_of(&s);
        let (slide, shape) = key_of(&ad, &doc, 2);
        let delete = op(OpKind::Structural, 0, Target { slide: Some(slide.clone()), ..Target::default() }, Payload::SlideDelete {});
        ad.apply(&mut s, 0, "a", &delete).expect("delete the slide");
        let before = doc_of(&s);

        let format = op(
            OpKind::Command,
            1,
            Target { slide: Some(slide), shape: Some(shape), ..Target::default() },
            format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(Some((0, 0)), None, None)]),
        );
        let err = ad.apply(&mut s, 1, "a", &format).expect_err("the slide is gone");
        assert_eq!(err.code, RejectCode::Stale);
        assert_eq!(doc_of(&s), before, "a dead address writes nothing");
    }

    /// The same deck with text in its first shape, so a run range has something to format.
    fn text_deck() -> Session {
        let mut s = deck();
        let id = doc_of(&s).slides[0].shapes[0].id;
        s.apply_remote("text.set", &json!({"id": id.0, "text": "Hello world"})).expect("text");
        s
    }

    /// `(run text, bold)` for every run of the first shape's paragraph.
    fn run_bolds(doc: &Presentation, shape: usize) -> Vec<(String, Option<bool>)> {
        doc.slides[0].shapes[shape]
            .text
            .as_ref()
            .expect("text")
            .paragraphs
            .iter()
            .flat_map(|p| p.runs.iter())
            .map(|r| (r.text.clone(), r.props.bold))
            .collect()
    }

    fn range(from: u32, to: u32) -> Option<deckcraft_collab_proto::TextRange> {
        Some(deckcraft_collab_proto::TextRange { from, to })
    }

    /// One address of a `format.*` operation.
    fn addr(cell: Option<(u32, u32)>, range: Option<deckcraft_collab_proto::TextRange>, caret: Option<u32>) -> FormatAddress {
        FormatAddress { cell, range, caret }
    }

    fn format_set(format: FormatCmd, args: Value, addresses: Vec<FormatAddress>) -> Payload {
        Payload::FormatSet { format, args, addresses }
    }

    #[test]
    fn two_clients_converge_on_formatting_a_run_range() {
        let (mut a, mut b) = (text_deck(), text_deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let (slide, shape) = key_of(&aa, &doc_of(&a), 0);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };
        let bold = op(OpKind::Command, 0, target, format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(None, range(0, 5), None)]));

        let da = aa.apply(&mut a, 0, "a", &bold).expect("apply on a");
        let db = bb.apply(&mut b, 0, "b", &bold).expect("apply on b");
        assert_eq!(da, db, "the same operation has the same delta on both replicas");
        assert_eq!(doc_of(&a), doc_of(&b), "the replicas converge");
        assert_eq!(doc_of(&a).slides[0].shapes[0].text.as_ref().map(|t| t.text()).as_deref(), Some("Hello world"), "the text is kept");
        assert_eq!(
            run_bolds(&doc_of(&a), 0),
            vec![("Hello".to_string(), Some(true)), (" world".to_string(), None)],
            "only the addressed runs are bold; the run boundary is where the range ends"
        );
    }

    #[test]
    fn undo_of_a_range_format_restores_the_previous_runs_and_leaves_the_rest() {
        let mut s = text_deck();
        // A pre-existing formatting on the whole body: the undo must put back the runs it
        // replaced (the italic stays, the bold goes) and must not touch the text after them.
        let id = doc_of(&s).slides[0].shapes[0].id;
        s.doc_mut().expect("doc").selection.shapes = vec![id];
        s.apply_remote("format.italic", &json!({})).expect("seed italic");
        let was = doc_of(&s).slides[0].shapes[0].text.clone();
        assert!(run_bolds(&doc_of(&s), 0).iter().all(|(_, bold)| bold.is_none()));

        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let (slide, shape) = key_of(&ad, &doc_of(&s), 0);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };
        let bold = op(OpKind::Command, 0, target, format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(None, range(0, 5), None)]));
        ad.apply(&mut s, 0, "a", &bold).expect("bold the range");
        assert_eq!(run_bolds(&doc_of(&s), 0), vec![("Hello".to_string(), Some(true)), (" world".to_string(), None)]);

        let undo = ad.undo(&s, 1, "a", "u-1").expect("undo a's range format");
        assert!(matches!(undo.payload, Payload::ShapeField { field: ShapeField::Text, .. }), "the exact body: {undo:?}");
        ad.apply(&mut s, 1, "a", &undo).expect("apply the undo");
        assert_eq!(doc_of(&s).slides[0].shapes[0].text.clone(), was, "the previous runs came back");
        assert!(run_bolds(&doc_of(&s), 0).iter().all(|(_, bold)| bold.is_none()), "the bold is gone");

        let redo = ad.redo(&s, 2, "a", "r-1").expect("redo a's range format");
        ad.apply(&mut s, 2, "a", &redo).expect("apply the redo");
        assert_eq!(
            run_bolds(&doc_of(&s), 0),
            vec![("Hello".to_string(), Some(true)), (" world".to_string(), None)],
            "the redo bolds exactly the range again"
        );
    }

    #[test]
    fn an_undo_of_a_range_format_is_refused_when_someone_else_changed_the_body() {
        let mut s = text_deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let (slide, shape) = key_of(&ad, &doc_of(&s), 0);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };
        let bold = op(OpKind::Command, 0, target.clone(), format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(None, range(0, 5), None)]));
        ad.apply(&mut s, 0, "a", &bold).expect("a bolds the range");
        let mut insert = op(OpKind::Command, 1, target, Payload::TextInsert { at: 11, text: "!".into(), cell: None });
        insert.op_id = "op-2".into();
        ad.apply(&mut s, 1, "b", &insert).expect("b types after the range");
        let after = doc_of(&s);

        let err = ad.undo(&s, 2, "a", "u-1").expect_err("b changed the same body");
        assert_eq!(err.code, RejectCode::UndoConflict);
        assert!(!err.retryable, "an undo never overwrites someone else's edit");
        assert_eq!(ad.history().get("a").map(|h| h.undo_len()), Some(1), "a refused undo moves nothing");
        assert_eq!(doc_of(&s), after, "and writes nothing");
    }

    #[test]
    fn a_run_range_outside_the_body_or_in_a_missing_cell_is_refused_before_any_write() {
        let mut s = text_deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let doc = doc_of(&s);
        let (slide, shape) = key_of(&ad, &doc, 0);
        let before = doc_of(&s);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };

        for bad in [range(50, 60), range(5, 5), range(9, 3), range(0, 12)] {
            let op = op(OpKind::Command, 0, target.clone(), format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(None, bad, None)]));
            let err = ad.apply(&mut s, 0, "a", &op).expect_err("a range the body does not have");
            assert_eq!(err.code, RejectCode::Invalid, "{bad:?}: {}", err.message);
            assert_eq!(doc_of(&s), before, "{bad:?}: a refused range writes nothing");
        }
        assert_eq!(ad.history().get("a").map(|h| h.undo_len()).unwrap_or(0), 0, "and records no step");

        // A table cell range past the cell's own body is refused too, and the neighbour is never
        // touched. Two replicas still converge on a valid range inside one cell.
        let (mut a, mut b) = (table_deck(), table_deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let (slide, shape) = key_of(&aa, &doc_of(&a), 2);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };
        let bad =
            op(OpKind::Command, 0, target.clone(), format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(Some((0, 0)), range(0, 9), None)]));
        let err = aa.apply(&mut a, 0, "a", &bad).expect_err("past the cell body");
        assert_eq!(err.code, RejectCode::Invalid);

        let bold = op(OpKind::Command, 0, target, format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(Some((0, 0)), range(0, 2), None)]));
        aa.apply(&mut a, 0, "a", &bold).expect("bold the first two characters");
        bb.apply(&mut b, 0, "b", &bold).expect("the other replica too");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(cell_text(&doc_of(&a), 2, 0, 0), "seed", "the cell text is kept");
        let bolds: Vec<Option<bool>> = cell_body(&doc_of(&a), 2, 0, 0).paragraphs.iter().flat_map(|p| p.runs.iter()).map(|r| r.props.bold).collect();
        assert_eq!(bolds, vec![Some(true), None], "only the addressed characters are bold");
        assert_eq!(cell_bold(&doc_of(&a), 2, 0, 1), None, "a neighbouring cell keeps its formatting");
    }

    #[test]
    fn a_remote_range_format_leaves_the_receiving_selection_alone() {
        let (mut a, mut b) = (text_deck(), text_deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let id = doc_of(&b).slides[0].shapes[0].id;
        let caret = TextSel { shape: id, anchor: (0, 11), caret: (0, 11), cell: None, notes: false, line_end: None };
        b.doc_mut().expect("doc").selection.text = Some(caret.clone());
        let (slide, shape) = key_of(&aa, &doc_of(&a), 0);
        let bold = op(
            OpKind::Command,
            0,
            Target { slide: Some(slide), shape: Some(shape), ..Target::default() },
            format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(None, range(0, 5), None)]),
        );
        aa.apply(&mut a, 0, "a", &bold).expect("a bolds");
        bb.apply(&mut b, 0, "b", &bold).expect("b applies a remote format");
        assert_eq!(b.doc().expect("doc").selection.text, Some(caret), "formatting moves no text, so b's caret stays put");
        assert_eq!(run_bolds(&doc_of(&b), 0), vec![("Hello".to_string(), Some(true)), (" world".to_string(), None)]);
    }

    /// The table at `shape` on the first slide.
    fn table_model(doc: &Presentation, shape: usize) -> &Table {
        match &doc.slides[0].shapes[shape].kind {
            ShapeKind::Table(t) => t,
            other => panic!("shape {shape} is not a table: {other:?}"),
        }
    }

    #[test]
    fn two_clients_converge_on_a_table_style_and_a_cell_fill() {
        let (mut a, mut b) = (table_deck(), table_deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let (slide, shape) = key_of(&aa, &doc_of(&a), 2);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };

        let style = op(OpKind::Command, 0, target.clone(), Payload::TableStyle { style: "grid".into() });
        aa.apply(&mut a, 0, "a", &style).expect("a sets the style");
        bb.apply(&mut b, 0, "b", &style).expect("b sets the style");
        assert_eq!(doc_of(&a), doc_of(&b), "the replicas converge");
        assert_eq!(table_model(&doc_of(&a), 2).style, "grid");

        // One final fill over one explicit cell: the neighbour is never written.
        let fill = Payload::TableCellFill { fill: CellFillSpec::Solid { color: "#ff0000".into() }, cells: vec![(0, 1)] };
        aa.apply(&mut a, 1, "a", &op(OpKind::Command, 1, target.clone(), fill.clone())).expect("a fills one cell");
        bb.apply(&mut b, 1, "b", &op(OpKind::Command, 1, target.clone(), fill)).expect("b fills one cell");
        assert_eq!(doc_of(&a), doc_of(&b));
        let doc = doc_of(&a);
        let table = table_model(&doc, 2);
        assert!(table.cell(0, 1).expect("cell").fill.is_some(), "the addressed cell is filled");
        assert!(table.cell(0, 0).expect("cell").fill.is_none(), "the neighbour is untouched");
        assert!(table.cell(1, 1).expect("cell").fill.is_none());

        // `Inherit` clears the explicit fill again and `None` sets an empty one.
        let clear = Payload::TableCellFill { fill: CellFillSpec::Inherit, cells: vec![(0, 1)] };
        aa.apply(&mut a, 2, "a", &op(OpKind::Command, 2, target.clone(), clear.clone())).expect("clear");
        bb.apply(&mut b, 2, "b", &op(OpKind::Command, 2, target.clone(), clear)).expect("clear");
        assert!(table_model(&doc_of(&a), 2).cell(0, 1).expect("cell").fill.is_none(), "no explicit fill");
        let empty = Payload::TableCellFill { fill: CellFillSpec::None, cells: vec![(0, 1)] };
        aa.apply(&mut a, 3, "a", &op(OpKind::Command, 3, target.clone(), empty.clone())).expect("none");
        bb.apply(&mut b, 3, "b", &op(OpKind::Command, 3, target, empty)).expect("none");
        assert!(matches!(table_model(&doc_of(&a), 2).cell(0, 1).expect("cell").fill, Some(deckcraft_model::Fill::None)));
        assert_eq!(doc_of(&a), doc_of(&b));
    }

    #[test]
    fn two_clients_converge_on_table_options_and_their_undo_restores_the_whole_table() {
        let (mut a, mut b) = (table_deck(), table_deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let (slide, shape) = key_of(&aa, &doc_of(&a), 2);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };
        let before = table_model(&doc_of(&a), 2).clone();

        // All six toggles are one final value: the table ends with exactly this set on both
        // replicas, including the flags that were already at their target and were not "toggled".
        let payload = Payload::TableOptions {
            header_row: false,
            total_row: true,
            banded_rows: false,
            first_column: true,
            last_column: false,
            banded_columns: true,
        };
        let envelope = op(OpKind::Command, 0, target.clone(), payload);
        aa.apply(&mut a, 0, "a", &envelope).expect("a sets the options");
        bb.apply_confirmed(&mut b, 0, "a", &envelope).expect("b applies the confirmed step");
        assert_eq!(doc_of(&a), doc_of(&b), "the replicas converge");
        let doc = doc_of(&a);
        let table = table_model(&doc, 2);
        assert_eq!(
            (table.first_row, table.last_row, table.band_row, table.first_col, table.last_col, table.band_col),
            (false, true, false, true, false, true)
        );

        // The undo image is the whole table, so one undo puts the previous six values back, and
        // the redo reapplies the exact set on both replicas.
        let undo = aa.undo(&a, 1, "a", "u-1").expect("undo");
        assert!(matches!(undo.payload, Payload::ShapeField { field: ShapeField::Table, .. }), "{undo:?}");
        aa.apply(&mut a, 1, "a", &undo).expect("a applies the undo");
        bb.apply_confirmed(&mut b, 1, "a", &undo).expect("b applies the confirmed undo");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(table_model(&doc_of(&a), 2), &before, "the first options before image is the original table");
        let redo = aa.redo(&a, 2, "a", "r-1").expect("redo");
        aa.apply(&mut a, 2, "a", &redo).expect("a applies the redo");
        bb.apply_confirmed(&mut b, 2, "a", &redo).expect("b applies the confirmed redo");
        assert_eq!(doc_of(&a), doc_of(&b));
        let doc = doc_of(&a);
        let table = table_model(&doc, 2);
        assert_eq!((table.first_row, table.last_row, table.band_row), (false, true, false));
        assert_eq!((table.first_col, table.last_col, table.band_col), (true, false, true));
    }

    #[test]
    fn two_clients_converge_on_a_smartart_value_and_its_undo_restores_the_whole_group() {
        let (mut a, mut b) = (smartart_deck(), smartart_deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let (slide, shape) = key_of(&aa, &doc_of(&a), 2);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };
        let before = doc_of(&a).slides[0].shapes[2].clone();
        let before_diagram = serde_json::to_value(before.diagram.as_ref().expect("diagram")).expect("diagram json");

        // One whole final diagram value: the layout, the outline, the style and the colours.
        let diagram = json!({
            "kind": "hierarchy",
            "nodes": [{"text": "CEO", "level": 0}, {"text": "CTO", "level": 1}, {"text": "Dev", "level": 2}],
            "style": "intense",
            "colors": "accent3",
        });
        let envelope = op(OpKind::Command, 0, target, Payload::SmartArtSet { diagram });
        aa.apply(&mut a, 0, "a", &envelope).expect("a edits the graphic");
        bb.apply_confirmed(&mut b, 0, "a", &envelope).expect("b applies the confirmed step");
        assert_eq!(doc_of(&a), doc_of(&b), "the replicas converge");
        let after = doc_of(&a).slides[0].shapes[2].clone();
        assert_eq!(after.diagram.as_ref().expect("diagram").kind, "hierarchy");
        assert_eq!(after.diagram.as_ref().expect("diagram").nodes.len(), 3);
        assert_ne!(serde_json::to_value(after.diagram.as_ref().expect("diagram")).expect("value"), before_diagram, "the outline changed too");

        // The undo image is the whole group, so one undo puts the outline, the generated shapes
        // and their ids back; the redo reapplies the exact stated value on both replicas.
        let undo = aa.undo(&a, 1, "a", "u-1").expect("undo");
        assert!(matches!(undo.payload, Payload::ShapeField { field: ShapeField::SmartArt, .. }), "{undo:?}");
        aa.apply(&mut a, 1, "a", &undo).expect("a applies the undo");
        bb.apply_confirmed(&mut b, 1, "a", &undo).expect("b applies the confirmed undo");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(doc_of(&a).slides[0].shapes[2], before, "the whole group came back, children included");
        let redo = aa.redo(&a, 2, "a", "r-1").expect("redo");
        aa.apply(&mut a, 2, "a", &redo).expect("a applies the redo");
        bb.apply_confirmed(&mut b, 2, "a", &redo).expect("b applies the confirmed redo");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(doc_of(&a).slides[0].shapes[2], after);
    }

    #[test]
    fn a_smartart_step_is_refused_on_a_plain_shape_and_an_undo_after_a_foreign_edit() {
        let mut s = smartart_deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let doc = doc_of(&s);
        let (slide, group) = key_of(&ad, &doc, 2);
        let (_, rect) = key_of(&ad, &doc, 0);
        let diagram = json!({"kind": "hierarchy", "nodes": [{"text": "CEO", "level": 0}]});
        let plain = op(
            OpKind::Command,
            0,
            Target { slide: Some(slide.clone()), shape: Some(rect), ..Target::default() },
            Payload::SmartArtSet { diagram: diagram.clone() },
        );
        let err = ad.apply(&mut s, 0, "a", &plain).expect_err("a rectangle is not a graphic");
        assert_eq!(err.code, RejectCode::Invalid);
        assert!(!err.retryable);
        assert_eq!(ad.history().get("a").map(|h| h.undo_len()).unwrap_or(0), 0, "a refusal records no step");

        // A foreign edit of the same graphic after Alice's step refuses her undo.
        let target = Target { slide: Some(slide), shape: Some(group), ..Target::default() };
        let first = op(OpKind::Command, 0, target.clone(), Payload::SmartArtSet { diagram });
        ad.apply(&mut s, 0, "a", &first).expect("a edits the graphic");
        let mut second =
            op(OpKind::Command, 1, target, Payload::SmartArtSet { diagram: json!({"kind": "cycle", "nodes": [{"text": "One", "level": 0}]}) });
        second.op_id = "op-2".into();
        ad.apply(&mut s, 1, "b", &second).expect("b edits the same graphic");
        let err = ad.undo(&s, 2, "a", "u-1").expect_err("b changed the graphic after Alice");
        assert_eq!(err.code, RejectCode::UndoConflict);
        assert_eq!(ad.history().get("a").map(|h| h.undo_len()), Some(1), "a refused undo moves nothing");
    }

    #[test]
    fn two_clients_converge_on_a_chart_value_and_its_undo_restores_the_whole_shape() {
        let (mut a, mut b) = (chart_deck(), chart_deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let (slide, shape) = key_of(&aa, &doc_of(&a), 2);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };
        let before = doc_of(&a).slides[0].shapes[2].clone();

        // One whole final chart value: a pie with new data and captions travels together, so no
        // replica merges the arguments against its own copy.
        let chart = json!({
            "kind": "pie",
            "categories": ["A", "B", "C"],
            "series": [{"name": "S", "values": [1.0, 2.0, 3.0]}],
            "title": "Revenue",
            "legend": "b",
        });
        let envelope = op(OpKind::Command, 0, target, Payload::ChartSet { chart });
        aa.apply(&mut a, 0, "a", &envelope).expect("a edits the chart");
        bb.apply_confirmed(&mut b, 0, "a", &envelope).expect("b applies the confirmed step");
        assert_eq!(doc_of(&a), doc_of(&b), "the replicas converge");
        let after = doc_of(&a).slides[0].shapes[2].clone();
        let ShapeKind::Chart(c) = &after.kind else { panic!("shape 2 is not a chart") };
        assert_eq!(c.kind, deckcraft_model::chart::ChartType::Pie);
        assert_eq!(c.categories, ["A", "B", "C"]);
        assert_eq!(c.title.as_deref(), Some("Revenue"));
        assert_eq!(c.series.len(), 1);

        // The undo image is the whole shape: one undo puts the prior type, data and options back
        // on both replicas, and the redo reapplies the exact stated value.
        let undo = aa.undo(&a, 1, "a", "u-1").expect("undo");
        assert!(matches!(undo.payload, Payload::ShapeField { field: ShapeField::Chart, .. }), "{undo:?}");
        aa.apply(&mut a, 1, "a", &undo).expect("a applies the undo");
        bb.apply_confirmed(&mut b, 1, "a", &undo).expect("b applies the confirmed undo");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(doc_of(&a).slides[0].shapes[2], before, "the whole chart shape came back");
        let redo = aa.redo(&a, 2, "a", "r-1").expect("redo");
        aa.apply(&mut a, 2, "a", &redo).expect("a applies the redo");
        bb.apply_confirmed(&mut b, 2, "a", &redo).expect("b applies the confirmed redo");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(doc_of(&a).slides[0].shapes[2], after);
    }

    #[test]
    fn a_chart_step_is_refused_on_a_plain_shape_and_an_undo_after_a_foreign_edit() {
        let mut s = chart_deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let doc = doc_of(&s);
        let (slide, chart) = key_of(&ad, &doc, 2);
        let (_, rect) = key_of(&ad, &doc, 0);
        let value = json!({"kind": "line", "categories": ["A"], "series": [{"name": "S", "values": [1.0]}]});
        let plain = op(
            OpKind::Command,
            0,
            Target { slide: Some(slide.clone()), shape: Some(rect), ..Target::default() },
            Payload::ChartSet { chart: value.clone() },
        );
        let err = ad.apply(&mut s, 0, "a", &plain).expect_err("a rectangle is not a chart");
        assert_eq!(err.code, RejectCode::Invalid);
        assert_eq!(ad.history().get("a").map(|h| h.undo_len()).unwrap_or(0), 0, "a refusal records no step");

        // A foreign edit of the same chart after Alice's step refuses her undo.
        let target = Target { slide: Some(slide), shape: Some(chart), ..Target::default() };
        let first = op(OpKind::Command, 0, target.clone(), Payload::ChartSet { chart: value });
        ad.apply(&mut s, 0, "a", &first).expect("a edits the chart");
        let mut second = op(
            OpKind::Command,
            1,
            target,
            Payload::ChartSet { chart: json!({"kind": "bar", "categories": ["B"], "series": [{"name": "T", "values": [2.0]}]}) },
        );
        second.op_id = "op-2".into();
        ad.apply(&mut s, 1, "b", &second).expect("b edits the same chart");
        let err = ad.undo(&s, 2, "a", "u-1").expect_err("b changed the chart after Alice");
        assert_eq!(err.code, RejectCode::UndoConflict);
    }

    #[test]
    fn a_chart_insertion_reaches_both_replicas_as_the_same_shape() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let chart = json!({"kind": "bar", "categories": ["A", "B"], "series": [{"name": "S", "values": [1.0, 2.0]}]});
        let target = Target { slide: Some(key_of(&aa, &doc_of(&a), 0).0), ..Target::default() };
        let envelope = op(OpKind::Structural, 0, target, Payload::ChartInsert { chart, rect: Some([50.0, 60.0, 300.0, 200.0]) });
        aa.apply(&mut a, 0, "a", &envelope).expect("a inserts the chart");
        bb.apply_confirmed(&mut b, 0, "a", &envelope).expect("b applies the confirmed insertion");
        assert_eq!(doc_of(&a), doc_of(&b), "both replicas hold the same chart");
        let inserted = &doc_of(&a).slides[0].shapes[2];
        let ShapeKind::Chart(c) = &inserted.kind else { panic!("the inserted shape is a chart") };
        assert_eq!(c.kind, deckcraft_model::chart::ChartType::Bar);
        assert_eq!(c.categories, ["A", "B"]);
        assert_eq!(c.series[0].values, [Some(1.0), Some(2.0)]);
        let xfrm = inserted.xfrm.expect("box");
        assert_eq!((xfrm.x, xfrm.y, xfrm.w, xfrm.h), (50.0, 60.0, 300.0, 200.0));

        // Its undo removes the chart again as one structural step, on both replicas.
        let undo = aa.undo(&a, 1, "a", "u-1").expect("undo the insertion");
        aa.apply(&mut a, 1, "a", &undo).expect("a applies the undo");
        bb.apply_confirmed(&mut b, 1, "a", &undo).expect("b applies the confirmed undo");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(doc_of(&a).slides[0].shapes.len(), 2, "the chart is gone");
    }

    #[test]
    fn a_smartart_insertion_reaches_both_replicas_as_the_same_graphic() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let diagram = json!({
            "kind": "hierarchy",
            "nodes": [{"text": "CEO", "level": 0}, {"text": "CTO", "level": 1}, {"text": "Dev", "level": 2}],
            "style": "intense",
            "colors": "accent3",
        });
        let target = Target { slide: Some(key_of(&aa, &doc_of(&a), 0).0), ..Target::default() };
        let envelope = op(OpKind::Structural, 0, target, Payload::SmartArtInsert { diagram, rect: Some([40.0, 50.0, 400.0, 300.0]) });
        aa.apply(&mut a, 0, "a", &envelope).expect("a inserts the graphic");
        bb.apply_confirmed(&mut b, 0, "a", &envelope).expect("b applies the confirmed insertion");
        assert_eq!(doc_of(&a), doc_of(&b), "both replicas hold the same drawing");
        let inserted = &doc_of(&a).slides[0].shapes[2];
        let d = inserted.diagram.as_ref().expect("a graphic");
        assert_eq!((d.kind.as_str(), d.style.as_str(), d.colors.as_str()), ("hierarchy", "intense", "accent3"));
        let texts: Vec<&str> = d.nodes.iter().map(|n| n.text.as_str()).collect();
        assert_eq!(texts, ["CEO", "CTO", "Dev"]);
        let xfrm = inserted.xfrm.expect("box");
        assert_eq!((xfrm.x, xfrm.y, xfrm.w, xfrm.h), (40.0, 50.0, 400.0, 300.0));
        match &inserted.kind {
            deckcraft_model::ShapeKind::Group { children, .. } => assert!(!children.is_empty(), "the drawing was generated"),
            other => panic!("the inserted shape is not a group: {other:?}"),
        }

        // Its undo removes the whole graphic as one structural step; the redo puts the whole shape
        // back (diagram, generated children and their ids) on both replicas.
        let undo = aa.undo(&a, 1, "a", "u-1").expect("undo the insertion");
        assert!(matches!(undo.payload, Payload::ShapeRestore { .. } | Payload::ShapeDelete {}), "{undo:?}");
        aa.apply(&mut a, 1, "a", &undo).expect("a applies the undo");
        bb.apply_confirmed(&mut b, 1, "a", &undo).expect("b applies the confirmed undo");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(doc_of(&a).slides[0].shapes.len(), 2, "the graphic is gone");
        let redo = aa.redo(&a, 2, "a", "r-1").expect("redo the insertion");
        aa.apply(&mut a, 2, "a", &redo).expect("a applies the redo");
        bb.apply_confirmed(&mut b, 2, "a", &redo).expect("b applies the confirmed redo");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(doc_of(&a).slides[0].shapes[2], *inserted, "the whole shape came back");
    }

    #[test]
    fn a_hostile_smartart_insertion_adds_nothing_and_records_no_step() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let before = doc_of(&s);
        let target = Target { slide: Some(key_of(&ad, &before, 0).0), ..Target::default() };
        for diagram in [
            json!({"kind": "nope", "nodes": []}),
            json!({"kind": "list", "nodes": [], "style": "nope"}),
            json!({"kind": "list", "nodes": [], "colors": "accent9"}),
            json!("not a diagram"),
        ] {
            let step = op(OpKind::Structural, 0, target.clone(), Payload::SmartArtInsert { diagram, rect: None });
            let err = ad.apply(&mut s, 0, "a", &step).expect_err("a hostile tree is refused");
            assert_eq!(err.code, RejectCode::Invalid, "{err:?}");
            assert!(!err.retryable);
        }
        assert_eq!(doc_of(&s), before, "a refusal changes nothing");
        assert_eq!(ad.history().get("a").map(|h| h.undo_len()).unwrap_or(0), 0, "a refusal records no step");
    }

    #[test]
    fn a_table_merge_and_split_converge_on_two_replicas() {
        let (mut a, mut b) = (table_deck(), table_deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let (slide, shape) = key_of(&aa, &doc_of(&a), 2);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };

        let merge = op(OpKind::Command, 0, target.clone(), Payload::TableMerge { from: (0, 0), to: (1, 1) });
        aa.apply(&mut a, 0, "a", &merge).expect("a merges");
        bb.apply(&mut b, 0, "b", &merge).expect("b merges");
        assert_eq!(doc_of(&a), doc_of(&b));
        let doc = doc_of(&a);
        let table = table_model(&doc, 2);
        assert_eq!((table.cell(0, 0).expect("anchor").row_span, table.cell(0, 0).expect("anchor").grid_span), (2, 2));
        assert!(table.cell(1, 1).expect("covered").is_covered(), "the other cells are covered");
        assert_eq!(table.cell(1, 0).expect("covered").text.text(), "", "an empty covered cell stays empty");

        let split = op(OpKind::Command, 1, target, Payload::TableSplit { cell: (0, 0) });
        aa.apply(&mut a, 1, "a", &split).expect("a splits");
        bb.apply(&mut b, 1, "b", &split).expect("b splits");
        assert_eq!(doc_of(&a), doc_of(&b));
        let doc = doc_of(&a);
        let table = table_model(&doc, 2);
        assert_eq!((table.cell(0, 0).expect("anchor").row_span, table.cell(0, 0).expect("anchor").grid_span), (1, 1));
        assert!(!table.cell(1, 1).expect("cell").is_covered(), "every cell is its own again");
        assert_eq!(table.cell(0, 0).expect("cell").text.text(), "seed", "the anchor keeps its text");
    }

    #[test]
    fn a_table_sizing_step_converges_on_two_replicas() {
        let (mut a, mut b) = (table_deck(), table_deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let (slide, shape) = key_of(&aa, &doc_of(&a), 2);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };
        let total = table_model(&doc_of(&a), 2).width();

        // Keeping the total, the next column gives the difference: the table's width is unchanged.
        let width = op(OpKind::Command, 0, target.clone(), Payload::TableColumnWidth { col: 0, width: 200.0, keep_total: true });
        aa.apply(&mut a, 0, "a", &width).expect("a widens the first column");
        bb.apply(&mut b, 0, "b", &width).expect("b widens the first column");
        assert_eq!(doc_of(&a), doc_of(&b));
        let doc = doc_of(&a);
        let table = table_model(&doc, 2);
        assert_eq!(table.cols[0], 200.0);
        assert_eq!(table.width(), total, "the neighbouring column took the difference");

        let height = op(OpKind::Command, 1, target, Payload::TableRowHeight { row: 1, height: 60.0 });
        aa.apply(&mut a, 1, "a", &height).expect("a sets the row height");
        bb.apply(&mut b, 1, "b", &height).expect("b sets the row height");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(table_model(&doc_of(&a), 2).rows[1].height, 60.0);
        let boxed = doc_of(&a).slides[0].shapes[2].xfrm.expect("box");
        assert_eq!(boxed.h, table_model(&doc_of(&a), 2).height(), "the box follows the rows");
    }

    #[test]
    fn undo_of_a_table_value_step_restores_the_whole_table_on_both_replicas() {
        let (mut a, mut b) = (table_deck(), table_deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let (slide, shape) = key_of(&aa, &doc_of(&a), 2);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };
        let before = table_model(&doc_of(&a), 2).clone();

        let steps = [
            Payload::TableStyle { style: "grid".into() },
            Payload::TableCellFill { fill: CellFillSpec::Solid { color: "#00ff00".into() }, cells: vec![(0, 0), (0, 1)] },
            Payload::TableColumnWidth { col: 1, width: 220.0, keep_total: false },
            Payload::TableRowHeight { row: 0, height: 30.0 },
            Payload::TableMerge { from: (0, 0), to: (1, 1) },
        ];
        for (i, payload) in steps.into_iter().enumerate() {
            let version = i as u64;
            let envelope = op(OpKind::Command, version, target.clone(), payload);
            aa.apply(&mut a, version, "a", &envelope).expect("a applies the step");
            bb.apply_confirmed(&mut b, version, "a", &envelope).expect("b applies the confirmed step");
        }
        assert_ne!(table_model(&doc_of(&a), 2), &before, "the table changed");

        // Every step's undo is one whole-table restore; a merge, a fill, a style and a size all
        // come back exactly, and the other replica applies the same confirmed command.
        for (i, version) in (5..=9).map(|v| v as u64).enumerate() {
            let undo = aa.undo(&a, version, "a", &format!("u-{i}")).expect("undo");
            assert!(matches!(undo.payload, Payload::ShapeField { field: ShapeField::Table, .. }), "{undo:?}");
            aa.apply(&mut a, version, "a", &undo).expect("a applies the undo");
            bb.apply_confirmed(&mut b, version, "a", &undo).expect("b applies the undo");
            assert_eq!(doc_of(&a), doc_of(&b), "the restore converges");
        }
        assert_eq!(table_model(&doc_of(&a), 2), &before, "the first step's before image is the original table");
    }

    #[test]
    fn a_foreign_table_edit_refuses_the_undo_of_a_table_value_step() {
        let mut s = table_deck();
        let id = doc_of(&s).slides[0].shapes[2].id;
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let (slide, shape) = key_of(&ad, &doc_of(&s), 2);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };
        ad.apply(&mut s, 0, "a", &op(OpKind::Command, 0, target, Payload::TableStyle { style: "grid".into() })).expect("a styles");
        // Someone else types in the table before the undo: the table is no longer what the step left.
        s.apply_remote("text.set", &json!({"id": id.0, "cell": [1, 1], "text": "bob"})).expect("bob types");
        let err = ad.undo(&s, 1, "a", "u-1").expect_err("the table changed");
        assert_eq!(err.code, RejectCode::UndoConflict);
        assert_eq!(ad.history().get("a").map(|h| h.undo_len()), Some(1), "a refused undo moves nothing");
        assert_eq!(table_model(&doc_of(&s), 2).style, "grid", "the style step is still applied");
    }

    #[test]
    fn a_table_value_step_is_validated_before_the_first_write() {
        let mut s = table_deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let (slide, shape) = key_of(&ad, &doc_of(&s), 2);
        let table = Target { slide: Some(slide.clone()), shape: Some(shape), ..Target::default() };
        let before = doc_of(&s);

        let cases: Vec<(&str, Payload)> = vec![
            ("a cell the table does not have", Payload::TableCellFill { fill: CellFillSpec::Inherit, cells: vec![(9, 9)] }),
            ("an empty cell list", Payload::TableCellFill { fill: CellFillSpec::Inherit, cells: Vec::new() }),
            ("a merge past the table", Payload::TableMerge { from: (0, 0), to: (2, 2) }),
            ("a one-cell merge", Payload::TableMerge { from: (0, 0), to: (0, 0) }),
            ("a split of a cell that is not merged", Payload::TableSplit { cell: (0, 0) }),
            ("a missing column", Payload::TableColumnWidth { col: 2, width: 100.0, keep_total: false }),
            ("a missing row", Payload::TableRowHeight { row: 9, height: 40.0 }),
            ("no style", Payload::TableStyle { style: String::new() }),
        ];
        for (what, payload) in cases {
            let err = ad.apply(&mut s, 0, "a", &op(OpKind::Command, 0, table.clone(), payload)).expect_err(what);
            assert_eq!(err.code, RejectCode::Invalid, "{what}: {}", err.message);
            assert_eq!(doc_of(&s), before, "{what}: a refused step writes nothing");
        }

        // A shape that is not a table, and a target without a shape, are refused too.
        let (slide_key, rect_key) = key_of(&ad, &doc_of(&s), 0);
        let not_a_table = Target { slide: Some(slide_key), shape: Some(rect_key), ..Target::default() };
        let err =
            ad.apply(&mut s, 0, "a", &op(OpKind::Command, 0, not_a_table, Payload::TableStyle { style: "grid".into() })).expect_err("not a table");
        assert_eq!(err.code, RejectCode::Invalid);
        let err = ad
            .apply(
                &mut s,
                0,
                "a",
                &op(OpKind::Command, 0, Target { slide: Some(slide), ..Target::default() }, Payload::TableStyle { style: "grid".into() }),
            )
            .expect_err("no shape");
        assert_eq!(err.code, RejectCode::Invalid);
        assert_eq!(doc_of(&s), before);
        assert_eq!(ad.history().get("a").map(|h| h.undo_len()).unwrap_or(0), 0, "no step was recorded");
    }

    #[test]
    fn a_table_row_edit_records_and_restores_the_whole_table() {
        let mut s = table_deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let doc = doc_of(&s);
        let (slide, shape) = key_of(&ad, &doc, 2);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };

        let edit = op(OpKind::Structural, 0, target, Payload::TableEdit { axis: TableAxis::Row, at: 0, insert: true });
        ad.apply(&mut s, 0, "a", &edit).expect("insert row");
        assert_eq!(table_rows(&doc_of(&s), 2), 3);
        assert_eq!(cell_text(&doc_of(&s), 2, 0, 0), "");
        assert_eq!(cell_text(&doc_of(&s), 2, 1, 0), "seed", "the old row moved down");

        let undo = ad.undo(&s, 1, "a", "u-1").expect("undo");
        assert!(matches!(undo.payload, Payload::ShapeField { field: ShapeField::Table, .. }), "{undo:?}");
        ad.apply(&mut s, 1, "a", &undo).expect("apply undo");
        assert_eq!(table_rows(&doc_of(&s), 2), 2, "the whole table came back");
        assert_eq!(cell_text(&doc_of(&s), 2, 0, 0), "seed");

        let redo = ad.redo(&s, 2, "a", "r-1").expect("redo");
        ad.apply(&mut s, 2, "a", &redo).expect("apply redo");
        assert_eq!(table_rows(&doc_of(&s), 2), 3);
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
            (format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(None, None, None)]), first.clone()),
            (format_set(FormatCmd::Size, json!({"size": 24.0}), vec![addr(None, None, None)]), second.clone()),
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
            format_set(FormatCmd::Bold, json!({"on": true, "ids": [99]}), vec![addr(None, None, None)]),
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

    /// A minimal binary glTF header: the magic, version 2 and a declared length.
    fn glb() -> Vec<u8> {
        let mut bytes = b"glTF".to_vec();
        bytes.extend(2u32.to_le_bytes());
        bytes.extend(20u32.to_le_bytes());
        bytes.extend([0u8; 8]);
        bytes
    }

    #[test]
    fn a_model_operation_runs_from_the_media_channel_on_both_replicas() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let bytes = glb();
        let sha = aa.media_mut().put("block.glb", bytes.clone()).expect("a model by its glTF signature");
        bb.media_mut().insert(&sha, bytes).expect("the same object, fetched before the operation");
        let slide = aa.keys().slide_key(doc_of(&a).slides[0].id).expect("slide").clone();

        // The kind in the operation and the stored object must agree: a picture digest is not a
        // model, and the refusal comes before any write.
        let picture = aa.media_mut().put("red.png", png()).expect("picture");
        let wrong = op(
            OpKind::Structural,
            0,
            Target { slide: Some(slide.clone()), ..Target::default() },
            Payload::MediaInsert { kind: MediaKind::Model, sha256: picture, name: "red.png".into(), rect: None },
        );
        let err = aa.apply(&mut a, 0, "a", &wrong).expect_err("a picture object is not a 3-D model");
        assert_eq!(err.code, RejectCode::Invalid);
        assert!(err.message.contains("picture"), "{}", err.message);

        let envelope = op(
            OpKind::Structural,
            0,
            Target { slide: Some(slide), ..Target::default() },
            Payload::MediaInsert { kind: MediaKind::Model, sha256: sha.clone(), name: "block.glb".into(), rect: Some([120.0, 90.0, 320.0, 240.0]) },
        );
        let da = aa.apply(&mut a, 0, "a", &envelope).expect("apply a");
        let db = bb.apply(&mut b, 0, "b", &envelope).expect("apply b");
        assert_eq!(da, db, "the same operation has the same delta on both replicas");
        assert_eq!(doc_of(&a), doc_of(&b), "both replicas hold the same model");
        let after = doc_of(&a);
        let model = after.slides[0].shapes.iter().find(|s| matches!(s.kind, ShapeKind::Model3d { .. })).expect("a 3-D model shape");
        assert_eq!(model.xfrm.expect("xfrm").x, 120.0);
        assert_eq!(model.descr, "block.glb");
        assert!(after.media.iter().any(|m| m.content_type == "model/gltf-binary"), "the part keeps the type the package writes");
        assert_eq!(aa.media().kind(&sha), Some(crate::MediaType::Model));
    }

    #[test]
    fn undo_of_a_model_insert_removes_it_from_both_replicas() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let bytes = glb();
        let sha = aa.media_mut().put("block.glb", bytes.clone()).expect("model");
        bb.media_mut().insert(&sha, bytes).expect("the same object");
        let before = doc_of(&a);
        let slide = aa.keys().slide_key(before.slides[0].id).expect("slide").clone();
        let insert = op(
            OpKind::Structural,
            0,
            Target { slide: Some(slide.clone()), ..Target::default() },
            Payload::MediaInsert { kind: MediaKind::Model, sha256: sha, name: "block.glb".into(), rect: None },
        );
        aa.apply(&mut a, 0, "a", &insert).expect("a inserts");
        bb.apply_confirmed(&mut b, 0, "a", &insert).expect("b inserts");
        let (_, inserted) = key_of(&aa, &doc_of(&a), 2);
        assert_eq!(aa.history().get("a").map(|h| h.undo_len()), Some(1), "a model insert is one undoable step");

        let undo = aa.undo(&a, 1, "a", "undo-model").expect("undo the insert");
        assert_eq!(undo.payload, Payload::ShapeDelete {});
        assert_eq!(undo.kind, OpKind::Structural);
        assert_eq!(undo.target.shapes, vec![inserted], "the delete names the inserted model");
        aa.apply(&mut a, 1, "a", &undo).expect("undo on a");
        bb.apply_confirmed(&mut b, 1, "a", &undo).expect("b applies the undo");
        assert_eq!(doc_of(&a), doc_of(&b), "both replicas converge on the undo");
        assert!(!doc_of(&a).slides[0].shapes.iter().any(|s| matches!(s.kind, ShapeKind::Model3d { .. })), "the model is gone");
        assert_eq!(doc_of(&a).slides[0].shapes, before.slides[0].shapes, "the slide is back to what it was");
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
        let bold = |op_id: &str, base: u64| {
            command(op_id, base, &slide, &first, format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(None, None, None)]))
        };
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
        let bold = command("op-1", 0, &slide, &first, format_set(FormatCmd::Bold, json!({"on": true}), vec![addr(None, None, None)]));
        let italic = command("op-2", 1, &slide, &first, format_set(FormatCmd::Italic, json!({"on": true}), vec![addr(None, None, None)]));
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

    fn transition(kind: &str, duration_ms: u32) -> Payload {
        Payload::SlideTransition { transition: Some(TransitionSpec { kind: kind.into(), duration_ms, ..TransitionSpec::default() }) }
    }

    fn transition_of(doc: &Presentation) -> Option<deckcraft_model::anim::Transition> {
        doc.slides[0].transition.clone()
    }

    #[test]
    fn a_transition_change_converges_on_both_replicas() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let slide = aa.keys().slide_key(doc_of(&a).slides[0].id).expect("slide").clone();

        let fade = op(OpKind::Command, 0, Target { slide: Some(slide), ..Target::default() }, transition("fade", 700));
        let da = aa.apply(&mut a, 0, "a", &fade).expect("apply a");
        let db = bb.apply(&mut b, 0, "b", &fade).expect("apply b");
        assert_eq!(da, db, "the same operation has the same delta on both replicas");
        assert_eq!(doc_of(&a), doc_of(&b), "both replicas hold the same transition");
        let t = transition_of(&doc_of(&a)).expect("a transition");
        assert_eq!((t.kind.as_str(), t.duration_ms), ("fade", 700));
        assert!(!da.is_empty(), "the slide's transition is a reported change");
    }

    #[test]
    fn a_second_participant_replaces_the_transition_and_the_first_undo_is_refused() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let slide = aa.keys().slide_key(doc_of(&a).slides[0].id).expect("slide").clone();

        let fade = op(OpKind::Command, 0, Target { slide: Some(slide.clone()), ..Target::default() }, transition("fade", 700));
        aa.apply(&mut a, 0, "a", &fade).expect("a fades");
        bb.apply_confirmed(&mut b, 0, "a", &fade).expect("b sees the fade");

        // Bob decided on the push before Alice's fade reached him (`baseVersion` 0, head 1): the
        // value is absolute, so nothing is rebased and the later accepted value wins.
        let push = op(OpKind::Command, 0, Target { slide: Some(slide.clone()), ..Target::default() }, transition("push", 1000));
        let da = aa.apply(&mut a, 1, "b", &push).expect("a sees b's push");
        let db = bb.apply(&mut b, 1, "b", &push).expect("b pushes");
        assert_eq!(da, db);
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(transition_of(&doc_of(&a)).map(|t| t.kind), Some("push".into()), "the later value wins");

        let err = aa.undo(&a, 2, "a", "undo-1").expect_err("undo would overwrite Bob's later change");
        assert_eq!(err.code, RejectCode::UndoConflict);
        assert_eq!(transition_of(&doc_of(&a)).map(|t| t.kind), Some("push".into()), "a refused undo changes nothing");

        // Bob's own undo puts back the fade his step replaced.
        let undo = bb.undo(&b, 2, "b", "undo-2").expect("b's undo");
        bb.apply(&mut b, 2, "b", &undo).expect("b restores");
        aa.apply_confirmed(&mut a, 2, "b", &undo).expect("a sees b's restore");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(transition_of(&doc_of(&a)).map(|t| t.kind), Some("fade".into()));
    }

    #[test]
    fn undo_of_a_transition_step_restores_the_whole_value_it_replaced_on_both_replicas() {
        let (mut a, mut b) = (deck(), deck());
        // Both replicas start from the same transition: an advance timing and a duration that
        // `transition.set` keeps while it changes only the effect.
        for s in [&mut a, &mut b] {
            s.apply_remote("transition.timing", &json!({"duration": 900, "after": 5000})).expect("timing");
        }
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let slide = aa.keys().slide_key(doc_of(&a).slides[0].id).expect("slide").clone();

        let fade = op(OpKind::Command, 0, Target { slide: Some(slide.clone()), ..Target::default() }, transition("fade", 700));
        aa.apply(&mut a, 0, "a", &fade).expect("a fades");
        bb.apply_confirmed(&mut b, 0, "a", &fade).expect("b sees the fade");
        assert_eq!(transition_of(&doc_of(&a)), transition_of(&doc_of(&b)));

        let undo = aa.undo(&a, 1, "a", "undo-1").expect("undo the transition");
        let Payload::SlideTransition { transition: Some(value) } = &undo.payload else { panic!("the whole previous transition") };
        assert_eq!((value.kind.as_str(), value.duration_ms, value.advance_after_ms), ("none", 900, Some(5000)));
        aa.apply(&mut a, 1, "a", &undo).expect("a restores");
        bb.apply_confirmed(&mut b, 1, "a", &undo).expect("b restores");
        assert_eq!(doc_of(&a), doc_of(&b), "the restore converges");
        let t = transition_of(&doc_of(&a)).expect("the timing keeps a transition");
        assert_eq!((t.kind.as_str(), t.duration_ms, t.advance_after_ms), ("none", 900, Some(5000)));

        let redo = aa.redo(&a, 2, "a", "redo-1").expect("redo the transition");
        aa.apply(&mut a, 2, "a", &redo).expect("a reapplies");
        bb.apply_confirmed(&mut b, 2, "a", &redo).expect("b reapplies");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(transition_of(&doc_of(&a)).map(|t| t.kind), Some("fade".into()));
    }

    #[test]
    fn a_transition_restore_cannot_name_another_value() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let slide = ad.keys().slide_key(doc_of(&s).slides[0].id).expect("slide").clone();
        ad.apply(&mut s, 0, "a", &op(OpKind::Command, 0, Target { slide: Some(slide.clone()), ..Target::default() }, transition("fade", 700)))
            .expect("fade");
        let mut undo = ad.undo(&s, 1, "a", "undo-1").expect("undo");
        undo.payload = transition("wipe", 1000);
        let err = ad.apply(&mut s, 1, "a", &undo).expect_err("a restore must repeat its recorded value");
        assert_eq!(err.code, RejectCode::Invalid);
        assert_eq!(transition_of(&doc_of(&s)).map(|t| t.kind), Some("fade".into()), "nothing was written");
    }

    #[test]
    fn an_unknown_effect_on_the_wire_is_refused() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let slide = ad.keys().slide_key(doc_of(&s).slides[0].id).expect("slide").clone();
        // An effect the model cannot name is accepted only with the XML a file kept for it.
        let err = ad
            .apply(&mut s, 0, "a", &op(OpKind::Command, 0, Target { slide: Some(slide), ..Target::default() }, transition("mystery", 1000)))
            .expect_err("unknown effect without kept XML");
        assert_eq!(err.code, RejectCode::Invalid);
        assert!(transition_of(&doc_of(&s)).is_none(), "nothing was written");
    }

    /// One animation step as the wire carries it; the payload's own shape id is deliberately
    /// another number, because the adapter forces the shape the target names.
    fn animation_step(effect: &str) -> Value {
        json!({"shape": 999, "effect": effect, "class": "entrance", "durationMs": 500})
    }

    fn animation_effects(doc: &Presentation) -> Vec<String> {
        doc.slides[0].animations.iter().map(|a| a.effect.clone()).collect()
    }

    fn step_list(doc: &Presentation) -> Vec<Value> {
        doc.slides[0].animations.iter().filter_map(|a| serde_json::to_value(a).ok()).collect()
    }

    #[test]
    fn animation_steps_converge_on_both_replicas_and_two_steps_in_a_row_keep_their_order() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let (slide, shape) = key_of(&aa, &doc_of(&a), 0);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };

        // `animation.add` normalised to the shape's whole list: no steps plus the new one.
        let fade = op(OpKind::Command, 0, target.clone(), Payload::ShapeAnimations { animations: vec![animation_step("fade")] });
        let da = aa.apply(&mut a, 0, "alice", &fade).expect("fade a");
        let db = bb.apply(&mut b, 0, "bob", &fade).expect("fade b");
        assert_eq!(da, db, "the same operation has the same delta on both replicas");
        assert_eq!(doc_of(&a), doc_of(&b), "the replicas converge");
        assert!(!da.is_empty(), "the slide's animation list is a reported change");
        assert_eq!(animation_effects(&doc_of(&a)), ["fade"]);

        // A second step in a row: the client sends the list it now has plus the new step.
        let mut steps = step_list(&doc_of(&a));
        steps.push(animation_step("pulse"));
        let pulse = op(OpKind::Command, 1, target.clone(), Payload::ShapeAnimations { animations: steps });
        aa.apply(&mut a, 1, "alice", &pulse).expect("pulse a");
        bb.apply_confirmed(&mut b, 1, "alice", &pulse).expect("pulse b");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(animation_effects(&doc_of(&a)), ["fade", "pulse"], "two steps in a row keep their order");
        assert!(doc_of(&a).slides[0].animations.iter().all(|x| x.shape.0 != 999), "the addressed shape wins over the value's id");
    }

    #[test]
    fn undo_of_an_animation_step_keeps_the_other_step_on_both_replicas() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let (slide, shape) = key_of(&aa, &doc_of(&a), 0);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };
        let fade = op(OpKind::Command, 0, target.clone(), Payload::ShapeAnimations { animations: vec![animation_step("fade")] });
        aa.apply(&mut a, 0, "alice", &fade).expect("fade");
        bb.apply_confirmed(&mut b, 0, "alice", &fade).expect("b sees the fade");
        let pulse = op(OpKind::Command, 1, target, Payload::ShapeAnimations { animations: vec![animation_step("fade"), animation_step("pulse")] });
        aa.apply(&mut a, 1, "alice", &pulse).expect("pulse");
        bb.apply_confirmed(&mut b, 1, "alice", &pulse).expect("b sees the pulse");
        assert_eq!(animation_effects(&doc_of(&a)), ["fade", "pulse"]);

        let undo = aa.undo(&a, 2, "alice", "undo-1").expect("undo the second step");
        let Payload::ShapeAnimations { animations } = &undo.payload else { panic!("the whole step list: {:?}", undo.payload) };
        assert_eq!(animations.len(), 1, "the step keeps the shape's other steps");
        aa.apply(&mut a, 2, "alice", &undo).expect("undo a");
        bb.apply_confirmed(&mut b, 2, "alice", &undo).expect("undo b");
        assert_eq!(doc_of(&a), doc_of(&b), "the restore converges");
        assert_eq!(animation_effects(&doc_of(&a)), ["fade"]);

        let undo = aa.undo(&a, 3, "alice", "undo-2").expect("undo the first step");
        aa.apply(&mut a, 3, "alice", &undo).expect("undo a");
        bb.apply_confirmed(&mut b, 3, "alice", &undo).expect("undo b");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert!(animation_effects(&doc_of(&a)).is_empty(), "the shape has no step again");

        let redo = aa.redo(&a, 4, "alice", "redo-1").expect("redo the first step");
        aa.apply(&mut a, 4, "alice", &redo).expect("redo a");
        bb.apply_confirmed(&mut b, 4, "alice", &redo).expect("redo b");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(animation_effects(&doc_of(&a)), ["fade"]);
    }

    #[test]
    fn an_animation_undo_is_refused_after_someone_else_changed_the_shape_steps() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let (slide, shape) = key_of(&aa, &doc_of(&a), 0);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };
        let fade = op(OpKind::Command, 0, target.clone(), Payload::ShapeAnimations { animations: vec![animation_step("fade")] });
        aa.apply(&mut a, 0, "alice", &fade).expect("fade");
        bb.apply_confirmed(&mut b, 0, "alice", &fade).expect("b sees the fade");

        // Bob adds his own step before Alice's undo; the list is absolute, so his value wins.
        let mine = op(OpKind::Command, 1, target.clone(), Payload::ShapeAnimations { animations: vec![animation_step("wipe")] });
        aa.apply(&mut a, 1, "bob", &mine).expect("bob's steps");
        bb.apply(&mut b, 1, "bob", &mine).expect("bob's steps");
        let err = aa.undo(&a, 2, "alice", "undo-1").expect_err("her list would overwrite Bob's step");
        assert_eq!(err.code, RejectCode::UndoConflict);
        assert!(!err.retryable);
        assert_eq!(animation_effects(&doc_of(&a)), ["wipe"], "a refused undo changes nothing");
    }

    #[test]
    fn an_animation_step_on_a_shape_that_cannot_be_addressed_is_refused_before_any_write() {
        let mut s = two_slide_deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let doc = doc_of(&s);
        let slide = ad.keys().slide_key(doc.slides[0].id).expect("slide").clone();
        let other = ad.keys().shape_key(doc.slides[1].id, doc.slides[1].shapes[0].id).expect("key").clone();

        for (why, target) in [
            ("an unknown key", Target { slide: Some(slide.clone()), shape: Some(ShapeKey("nope".into())), ..Target::default() }),
            ("a shape of another slide", Target { slide: Some(slide.clone()), shape: Some(other), ..Target::default() }),
            ("no shape at all", Target { slide: Some(slide), ..Target::default() }),
        ] {
            let step = op(OpKind::Command, 0, target, Payload::ShapeAnimations { animations: vec![animation_step("fade")] });
            let err = ad.apply(&mut s, 0, "a", &step).expect_err(why);
            assert!(matches!(err.code, RejectCode::Invalid | RejectCode::Stale), "{why}: {err:?}");
            assert!(animation_effects(&doc_of(&s)).is_empty(), "{why} wrote a step");
        }

        // A value the model cannot read is refused too, and the list is all-or-nothing.
        let doc = doc_of(&s);
        let slide = ad.keys().slide_key(doc.slides[0].id).expect("slide").clone();
        let (_, shape) = key_of(&ad, &doc, 0);
        let step = op(
            OpKind::Command,
            0,
            Target { slide: Some(slide), shape: Some(shape), ..Target::default() },
            Payload::ShapeAnimations { animations: vec![animation_step("fade"), json!({"effect": "mystery"})] },
        );
        let err = ad.apply(&mut s, 0, "a", &step).expect_err("an unknown effect");
        assert!(matches!(err.code, RejectCode::Invalid | RejectCode::Unsupported), "{err:?}");
        assert!(animation_effects(&doc_of(&s)).is_empty(), "nothing of a refused list was written");
    }

    #[test]
    fn an_animation_restore_cannot_name_another_list() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let (slide, shape) = key_of(&ad, &doc_of(&s), 0);
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };
        ad.apply(&mut s, 0, "a", &op(OpKind::Command, 0, target, Payload::ShapeAnimations { animations: vec![animation_step("fade")] }))
            .expect("fade");
        let mut undo = ad.undo(&s, 1, "a", "undo-1").expect("undo");
        undo.payload = Payload::ShapeAnimations { animations: vec![animation_step("wipe")] };
        let err = ad.apply(&mut s, 1, "a", &undo).expect_err("a restore must repeat its recorded value");
        assert_eq!(err.code, RejectCode::Invalid);
        assert_eq!(animation_effects(&doc_of(&s)), ["fade"], "nothing was written");
    }

    /// Seed speaker notes through the single-user command, so both replicas start equal.
    fn seed_notes(s: &mut Session, text: &str) {
        s.apply_remote("slide.notes", &json!({"index": 0, "text": text})).expect("notes");
    }

    fn notes_text(doc: &Presentation, index: usize) -> String {
        doc.slides[index].notes.text()
    }

    fn notes_key(adapter: &Adapter, doc: &Presentation) -> SlideKey {
        adapter.keys().slide_key(doc.slides[0].id).expect("slide key").clone()
    }

    #[test]
    fn a_notes_edit_converges_on_both_replicas_after_a_slide_is_inserted() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let slide = notes_key(&aa, &doc_of(&a));

        // A slide is inserted first: structural keys stay put, so the notes address still names
        // the same slide on both replicas.
        let new_slide = op(
            OpKind::Structural,
            0,
            Target { slide: Some(slide.clone()), ..Target::default() },
            Payload::SlideInsert { before: None, layout: Some("blank".into()) },
        );
        aa.apply(&mut a, 0, "bob", &new_slide).expect("insert a");
        bb.apply(&mut b, 0, "bob", &new_slide).expect("insert b");
        assert_eq!(doc_of(&a).slides.len(), 2);

        // Typing in the first slide's notes, based on the version before the insert arrived.
        let typing =
            op(OpKind::Command, 0, Target { slide: Some(slide), ..Target::default() }, Payload::NotesInsert { at: 0, text: "Agenda".into() });
        let da = aa.apply(&mut a, 1, "alice", &typing).expect("typing a");
        bb.apply(&mut b, 1, "alice", &typing).expect("typing b");
        assert_eq!(doc_of(&a), doc_of(&b), "the replicas converge");
        assert_eq!(notes_text(&doc_of(&a), 0), "Agenda");
        assert_eq!(notes_text(&doc_of(&a), 1), "", "the slide inserted later was not the one edited");
        assert!(!da.is_empty(), "the slide's own change is reported");
    }

    #[test]
    fn undo_of_a_notes_step_restores_the_previous_text_and_its_formatting() {
        let (mut a, mut b) = (deck(), deck());
        for s in [&mut a, &mut b] {
            seed_notes(s, "old");
            // A bold heading over the notes: the image must carry the runs, not only the text.
            s.apply_remote("text.edit", &json!({"notes": true})).expect("edit notes");
            s.apply_remote("text.selectAll", &json!({})).expect("select");
            s.apply_remote("format.bold", &json!({"on": true})).expect("bold");
        }
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let target = Target { slide: Some(notes_key(&aa, &doc_of(&a))), ..Target::default() };

        let typing = op(OpKind::Command, 0, target, Payload::NotesInsert { at: 3, text: " notes".into() });
        aa.apply(&mut a, 0, "alice", &typing).expect("type a");
        bb.apply_confirmed(&mut b, 0, "alice", &typing).expect("type b");
        assert_eq!(notes_text(&doc_of(&a), 0), "old notes");
        assert_eq!(doc_of(&a), doc_of(&b));

        let undo = aa.undo(&a, 1, "alice", "undo-1").expect("undo the notes step");
        let Payload::NotesSet { notes } = &undo.payload else { panic!("the whole body: {:?}", undo.payload) };
        let image: TextBody = serde_json::from_value(notes.clone()).expect("a text body");
        assert_eq!(image.text(), "old", "the image is the body the edit replaced");
        aa.apply(&mut a, 1, "alice", &undo).expect("undo a");
        bb.apply_confirmed(&mut b, 1, "alice", &undo).expect("undo b");
        assert_eq!(doc_of(&a), doc_of(&b), "the restore converges");
        assert_eq!(notes_text(&doc_of(&a), 0), "old");
        assert_eq!(doc_of(&a).slides[0].notes.paragraphs[0].runs[0].props.bold, Some(true), "the runs came back with the body");

        let redo = aa.redo(&a, 2, "alice", "redo-1").expect("redo");
        aa.apply(&mut a, 2, "alice", &redo).expect("redo a");
        bb.apply_confirmed(&mut b, 2, "alice", &redo).expect("redo b");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(notes_text(&doc_of(&a), 0), "old notes");
    }

    #[test]
    fn a_notes_undo_is_refused_after_someone_else_edited_the_notes() {
        let (mut a, mut b) = (deck(), deck());
        seed_notes(&mut a, "old");
        seed_notes(&mut b, "old");
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let target = Target { slide: Some(notes_key(&aa, &doc_of(&a))), ..Target::default() };

        let typing = op(OpKind::Command, 0, target.clone(), Payload::NotesInsert { at: 0, text: "A".into() });
        aa.apply(&mut a, 0, "alice", &typing).expect("alice");
        bb.apply_confirmed(&mut b, 0, "alice", &typing).expect("b sees alice");
        let theirs = op(OpKind::Command, 1, target, Payload::NotesInsert { at: 0, text: "B".into() });
        aa.apply(&mut a, 1, "bob", &theirs).expect("bob");
        bb.apply(&mut b, 1, "bob", &theirs).expect("bob");

        let err = aa.undo(&a, 2, "alice", "undo-1").expect_err("her body is not the recorded one any more");
        assert_eq!(err.code, RejectCode::UndoConflict);
        assert!(!err.retryable);
        assert_eq!(notes_text(&doc_of(&a), 0), "BAold", "a refused undo changes nothing");
    }

    #[test]
    fn a_notes_step_on_a_slide_that_cannot_be_addressed_is_refused_before_any_write() {
        let mut s = two_slide_deck();
        seed_notes(&mut s, "keep");
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let doc = doc_of(&s);
        let first = ad.keys().slide_key(doc.slides[0].id).expect("slide").clone();
        let second = doc.slides[1].id;

        // A deleted slide keeps its key as a tombstone: a late notes write is refused, and the
        // body of the slide that is left is untouched.
        let second_key = ad.keys().slide_key(second).expect("second").clone();
        ad.apply(
            &mut s,
            0,
            "a",
            &op(OpKind::Structural, 0, Target { slide: Some(second_key.clone()), ..Target::default() }, Payload::SlideDelete {}),
        )
        .expect("delete the second slide");
        let err = ad
            .apply(
                &mut s,
                1,
                "a",
                &op(OpKind::Command, 1, Target { slide: Some(second_key), ..Target::default() }, Payload::NotesInsert { at: 0, text: "x".into() }),
            )
            .expect_err("a deleted slide");
        assert!(matches!(err.code, RejectCode::Stale | RejectCode::Invalid), "{err:?}");

        for (why, target) in
            [("an unknown key", Target { slide: Some(SlideKey("nope".into())), ..Target::default() }), ("no slide at all", Target::default())]
        {
            let err =
                ad.apply(&mut s, 1, "a", &op(OpKind::Command, 1, target, Payload::NotesSet { notes: json!({"paragraphs": []}) })).expect_err(why);
            assert!(matches!(err.code, RejectCode::Stale | RejectCode::Invalid), "{why}: {err:?}");
        }

        // Notes belong to a slide: a shape key in the target is refused, and so is a body this
        // build cannot read.
        let (_, shape) = key_of(&ad, &doc_of(&s), 0);
        let err = ad
            .apply(
                &mut s,
                1,
                "a",
                &op(
                    OpKind::Command,
                    1,
                    Target { slide: Some(first.clone()), shape: Some(shape), ..Target::default() },
                    Payload::NotesInsert { at: 0, text: "x".into() },
                ),
            )
            .expect_err("a shape is not a notes address");
        assert_eq!(err.code, RejectCode::Invalid);
        let err = ad
            .apply(
                &mut s,
                1,
                "a",
                &op(OpKind::Command, 1, Target { slide: Some(first), ..Target::default() }, Payload::NotesSet { notes: json!("not a body") }),
            )
            .expect_err("an unreadable body");
        assert_eq!(err.code, RejectCode::Invalid);
        assert_eq!(notes_text(&doc_of(&s), 0), "keep", "nothing of a refused notes step was written");
    }

    #[test]
    fn a_remote_notes_edit_keeps_the_receiving_caret_and_selection() {
        let (mut a, mut b) = (deck(), deck());
        for s in [&mut a, &mut b] {
            seed_notes(s, "old");
        }
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let target = Target { slide: Some(notes_key(&aa, &doc_of(&a))), ..Target::default() };

        // Bob is typing at the end of the same notes body.
        b.doc_mut().expect("doc").selection.text =
            Some(TextSel { shape: ShapeId(0), anchor: (0, 3), caret: (0, 3), cell: None, notes: true, line_end: None });
        let typing = op(OpKind::Command, 0, target.clone(), Payload::NotesInsert { at: 0, text: "X ".into() });
        aa.apply(&mut a, 0, "alice", &typing).expect("alice");
        bb.apply_confirmed(&mut b, 0, "alice", &typing).expect("b applies it");
        assert_eq!(notes_text(&doc_of(&b), 0), "X old");
        let caret = b.doc().expect("doc").selection.text.as_ref().expect("caret");
        assert!(caret.notes, "the caret stays in the notes");
        assert_eq!((caret.anchor, caret.caret), ((0, 5), (0, 5)), "it moved with the text before it, not to the end");

        // A deletion before the caret pulls it left as well.
        let deleting = op(OpKind::Command, 1, target, Payload::NotesDelete { from: 0, to: 2 });
        aa.apply(&mut a, 1, "alice", &deleting).expect("alice deletes");
        bb.apply_confirmed(&mut b, 1, "alice", &deleting).expect("b applies the delete");
        assert_eq!(notes_text(&doc_of(&b), 0), "old");
        let caret = b.doc().expect("doc").selection.text.as_ref().expect("caret");
        assert_eq!((caret.anchor, caret.caret), ((0, 3), (0, 3)), "the caret follows the deleted prefix");

        // A selection in a shape's text on another slide is left exactly as it was.
        let shape = doc_of(&a).slides[0].shapes[0].id;
        bb.apply_confirmed(
            &mut b,
            2,
            "alice",
            &op(
                OpKind::Command,
                2,
                Target { slide: Some(notes_key(&aa, &doc_of(&a))), ..Target::default() },
                Payload::NotesInsert { at: 3, text: "!".into() },
            ),
        )
        .expect("another notes edit");
        b.doc_mut().expect("doc").selection.text = Some(TextSel { shape, anchor: (0, 1), caret: (0, 1), cell: None, notes: false, line_end: None });
        let more = op(
            OpKind::Command,
            3,
            Target { slide: Some(notes_key(&aa, &doc_of(&a))), ..Target::default() },
            Payload::NotesInsert { at: 0, text: "?".into() },
        );
        bb.apply_confirmed(&mut b, 3, "alice", &more).expect("a third notes edit");
        let sel = b.doc().expect("doc").selection.clone();
        let text = sel.text.as_ref().expect("shape selection");
        assert!(!text.notes, "a shape's text selection is not the notes");
        assert_eq!((sel.slide, text.anchor, text.caret), (0, (0, 1), (0, 1)));
        assert_eq!(
            notes_text(&doc_of(&b), 0),
            "?Old!",
            "the single-user autocorrect capitalises the sentence start, and every replica does the same"
        );
    }

    #[test]
    fn a_notes_restore_cannot_name_another_body() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let target = Target { slide: Some(notes_key(&ad, &doc_of(&s))), ..Target::default() };
        ad.apply(&mut s, 0, "a", &op(OpKind::Command, 0, target, Payload::NotesInsert { at: 0, text: "A".into() })).expect("type");
        let mut undo = ad.undo(&s, 1, "a", "undo-1").expect("undo");
        undo.payload = Payload::NotesSet { notes: json!({"paragraphs": [{"runs": [{"text": "another"}]}]}) };
        let err = ad.apply(&mut s, 1, "a", &undo).expect_err("a restore must repeat its recorded body");
        assert_eq!(err.code, RejectCode::Invalid);
        assert_eq!(notes_text(&doc_of(&s), 0), "A", "nothing was written");

        // A body that is not a whole text body is refused even on the ordinary edit path.
        let err =
            ad.apply(&mut s, 1, "a", &op(OpKind::Command, 1, Target::default(), Payload::NotesSet { notes: json!(42) })).expect_err("not a body");
        assert!(matches!(err.code, RejectCode::Invalid | RejectCode::Stale), "{err:?}");
    }

    /// A small original font the model carries and the font database reads (ColrSquares).
    fn font_bytes() -> Vec<u8> {
        include_bytes!("../../render/tests/fixtures/colr-squares.ttf").to_vec()
    }

    /// The document's entry for the test typeface, if it has one.
    fn font_entry(doc: &Presentation) -> Option<deckcraft_model::EmbeddedFont> {
        doc.embedded_fonts.iter().find(|f| f.typeface == "Deck Test Face").cloned()
    }

    /// `(adapter, session)` for one replica that holds the font bytes on its media channel.
    fn font_replica() -> (Adapter, Session) {
        let s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let bytes = font_bytes();
        ad.media_mut().insert(&crate::media::sha256_hex(&bytes), bytes).expect("media");
        (ad, s)
    }

    fn font_create(sha: &str) -> Payload {
        Payload::FontCreate {
            typeface: "Deck Test Face".into(),
            face: deckcraft_collab_proto::FontFace::Regular,
            sha256: sha.into(),
            name: "DeckFace.ttf".into(),
        }
    }

    #[test]
    fn two_clients_converge_on_an_embedded_font_and_its_descriptors() {
        let ((mut aa, mut a), (mut bb, mut b)) = (font_replica(), font_replica());
        let sha = crate::media::sha256_hex(&font_bytes());
        let create = op(OpKind::Command, 0, Target::default(), font_create(&sha));

        let da = aa.apply(&mut a, 0, "a", &create).expect("a embeds the font");
        let db = bb.apply(&mut b, 0, "b", &create).expect("b applies the confirmed command");
        assert_eq!(da, db, "the same operation has the same delta on both replicas");
        assert_eq!(doc_of(&a), doc_of(&b), "the replicas converge, media objects included");
        let entry = font_entry(&doc_of(&a)).expect("the entry");
        assert_eq!(entry.typeface, "Deck Test Face");
        let id = entry.regular.expect("the regular face points at an object");
        assert_eq!(doc_of(&a).media(id).map(|m| m.content_type.as_str()), Some("application/x-fontdata"));
        assert!(doc_of(&b).media(id).is_some(), "the same object id exists on the other replica");

        // A descriptor step is one absolute value: the later accepted one wins on both.
        let mut set = op(
            OpKind::Command,
            1,
            Target::default(),
            Payload::FontSet {
                typeface: "Deck Test Face".into(),
                panose: Some("020B0604020202020204".into()),
                pitch_family: Some(2),
                charset: Some(204),
            },
        );
        set.op_id = "op-2".into();
        let da = aa.apply(&mut a, 1, "a", &set).expect("a sets the descriptors");
        let db = bb.apply(&mut b, 1, "b", &set).expect("b applies the confirmed command");
        assert_eq!(da, db);
        assert_eq!(doc_of(&a), doc_of(&b));
        let entry = font_entry(&doc_of(&a)).expect("the entry");
        assert_eq!(entry.panose.as_deref(), Some("020B0604020202020204"));
        assert_eq!((entry.pitch_family, entry.charset), (Some(2), Some(204)));
        assert_eq!(entry.regular, Some(id), "the face is untouched by a descriptor step");

        // And removal reaches both, leaving the face object for an undo image that names it.
        let mut remove = op(OpKind::Command, 2, Target::default(), Payload::FontRemove { typeface: "Deck Test Face".into() });
        remove.op_id = "op-3".into();
        aa.apply(&mut a, 2, "a", &remove).expect("a removes the entry");
        bb.apply(&mut b, 2, "b", &remove).expect("b applies the confirmed command");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert!(font_entry(&doc_of(&a)).is_none());
        assert!(doc_of(&a).media(id).is_some(), "the face object stays in the media list");
    }

    #[test]
    fn an_undo_of_a_document_font_restores_the_whole_entry() {
        let ((mut aa, mut a), (mut bb, mut b)) = (font_replica(), font_replica());
        let sha = crate::media::sha256_hex(&font_bytes());
        let create = op(OpKind::Command, 0, Target::default(), font_create(&sha));
        aa.apply(&mut a, 0, "a", &create).expect("create");
        bb.apply(&mut b, 0, "a", &create).expect("the confirmed command on the second replica");

        let mut set = op(
            OpKind::Command,
            1,
            Target::default(),
            Payload::FontSet { typeface: "Deck Test Face".into(), panose: Some("020B0604020202020204".into()), pitch_family: None, charset: None },
        );
        set.op_id = "op-2".into();
        aa.apply(&mut a, 1, "a", &set).expect("set");
        bb.apply(&mut b, 1, "a", &set).expect("set on the second replica");
        assert_eq!(font_entry(&doc_of(&a)).and_then(|e| e.panose), Some("020B0604020202020204".into()));

        // Undo of the descriptor step is the whole entry before it: no slide is named.
        let undo = aa.undo(&a, 2, "a", "u-1").expect("undo the set");
        assert_eq!(undo.target.slide, None, "a document font is presentation-level");
        assert!(matches!(&undo.payload, Payload::FontRestore { font: Some(_), .. }), "{:?}", undo.payload);
        aa.apply(&mut a, 2, "a", &undo).expect("apply the undo");
        bb.apply(&mut b, 2, "a", &undo).expect("apply the undo on the second replica");
        assert_eq!(doc_of(&a), doc_of(&b), "both replicas put the same entry back");
        assert_eq!(font_entry(&doc_of(&a)).and_then(|e| e.panose), None, "the descriptor step is gone");
        assert!(font_entry(&doc_of(&a)).is_some(), "the entry and its face stay");

        // Undo of the create itself removes the entry on both; its object stays for the redo.
        let undo = aa.undo(&a, 3, "a", "u-2").expect("undo the create");
        assert!(matches!(&undo.payload, Payload::FontRestore { font: None, .. }), "{:?}", undo.payload);
        aa.apply(&mut a, 3, "a", &undo).expect("apply the undo");
        bb.apply(&mut b, 3, "a", &undo).expect("apply the undo on the second replica");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert!(font_entry(&doc_of(&a)).is_none());
        assert_eq!(doc_of(&a).media.len(), doc_of(&b).media.len(), "the face object stays on both");

        // Redo of the create puts the whole entry back.
        let redo = aa.redo(&a, 4, "a", "r-1").expect("redo the create");
        aa.apply(&mut a, 4, "a", &redo).expect("apply the redo");
        bb.apply(&mut b, 4, "a", &redo).expect("apply the redo on the second replica");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert!(font_entry(&doc_of(&a)).is_some_and(|e| e.regular.is_some()));
    }

    #[test]
    fn an_undo_of_a_document_font_is_refused_after_someone_else_changed_the_entry() {
        let (mut ad, mut s) = font_replica();
        let sha = crate::media::sha256_hex(&font_bytes());
        ad.apply(&mut s, 0, "a", &op(OpKind::Command, 0, Target::default(), font_create(&sha))).expect("a embeds");
        let mut set = op(
            OpKind::Command,
            1,
            Target::default(),
            Payload::FontSet { typeface: "Deck Test Face".into(), charset: Some(1), panose: None, pitch_family: None },
        );
        set.op_id = "op-2".into();
        ad.apply(&mut s, 1, "b", &set).expect("b changes the entry");

        let err = ad.undo(&s, 2, "a", "u-1").expect_err("b changed the entry after a's step");
        assert_eq!(err.code, RejectCode::UndoConflict);
        assert!(!err.retryable);
        assert_eq!(ad.history.get("a").map(ActorHistory::undo_len), Some(1), "a refused undo moves nothing");
        assert_eq!(font_entry(&doc_of(&s)).and_then(|e| e.charset), Some(1), "b's value stays");
    }

    #[test]
    fn a_font_step_is_validated_before_the_first_write() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let bytes = font_bytes();
        let sha = crate::media::sha256_hex(&bytes);
        let doc = doc_of(&s);

        // The bytes are not on this replica's media channel: retryable, nothing written.
        let err = ad.apply(&mut s, 0, "a", &op(OpKind::Command, 0, Target::default(), font_create(&sha))).expect_err("no bytes yet");
        assert!(err.retryable, "fetching the object and resending can succeed");
        assert_eq!(doc_of(&s), doc);

        // A picture digest is not a font.
        let png = png();
        let png_sha = crate::media::sha256_hex(&png);
        ad.media_mut().insert(&png_sha, png).expect("media");
        let err = ad.apply(&mut s, 0, "a", &op(OpKind::Command, 0, Target::default(), font_create(&png_sha))).expect_err("a picture");
        assert_eq!(err.code, RejectCode::Invalid);
        assert_eq!(doc_of(&s), doc);

        // A document font addressed by a slide is refused.
        let (slide, _shape) = key_of(&ad, &doc, 0);
        ad.media_mut().insert(&sha, bytes.clone()).expect("media");
        let err = ad
            .apply(&mut s, 0, "a", &op(OpKind::Command, 0, Target { slide: Some(slide), ..Target::default() }, font_create(&sha)))
            .expect_err("a font has no slide");
        assert_eq!(err.code, RejectCode::Invalid);

        // An unknown entry cannot be described or removed, and a set with no descriptor is refused.
        let unknown = |payload| op(OpKind::Command, 0, Target::default(), payload);
        let err = ad
            .apply(&mut s, 0, "a", &unknown(Payload::FontSet { typeface: "Nope".into(), panose: None, pitch_family: None, charset: Some(1) }))
            .expect_err("no such entry");
        assert_eq!(err.code, RejectCode::Invalid);
        let err = ad.apply(&mut s, 0, "a", &unknown(Payload::FontRemove { typeface: "Nope".into() })).expect_err("no such entry");
        assert_eq!(err.code, RejectCode::Invalid);
        let err = ad
            .apply(&mut s, 0, "a", &unknown(Payload::FontSet { typeface: "Deck Test Face".into(), panose: None, pitch_family: None, charset: None }))
            .expect_err("nothing to set");
        assert_eq!(err.code, RejectCode::Invalid);
        assert!(doc_of(&s).embedded_fonts.is_empty(), "no refused step wrote an entry");
        assert_eq!(doc_of(&s).media.len(), 0, "and no object was added");
    }
}
