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
use deckcraft_collab_proto::{ChangeKind, Delta, HistoryAction, MediaKind, OpEnvelope, OpKind, Payload, RejectCode, ShapeKey, SlideKey, Target};
use deckcraft_engine::{Session, Target as EngineTarget, TextSel};
use deckcraft_model::text::TextBody;
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind, SlideId, find_shape, parent_of};
use serde_json::{Value, json};

use crate::delta::delta;
use crate::history::{Histories, Image, Snapshot, Step};
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

    pub fn keys(&self) -> &KeyMap {
        &self.keys
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

        let before = session.doc().map_err(|_| Rejection::invalid("no document open", Some(head)))?.doc.clone();
        // What this step replaces, for the actor's own undo (ADR 0010). Captured only for
        // operations that are a merge of one object's values; a structural step is not undone in
        // v1 (RFC §13, open question 4).
        let old_image = self.capture(&before, op);

        let out = self.apply_confirmed(session, head, actor, op)?;
        let after = session.doc().map_err(|_| Rejection::invalid("the document closed while applying", Some(head)))?.doc.clone();

        match &op.history {
            Some(HistoryAction::Undo { .. }) => self.history.of(actor).commit_undo(),
            Some(HistoryAction::Redo { .. }) => self.history.of(actor).commit_redo(),
            None => {
                if let Some(old) = old_image
                    && let Some(new) = self.capture(&after, op)
                {
                    let slot = self.history.of(actor);
                    slot.record(Step { op_id: op.op_id.clone(), version: head.saturating_add(1), before: old, after: new });
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
        Ok(out)
    }

    /// Build the operation that takes back `actor`'s last step (ADR 0010).
    ///
    /// Refused with `undo_conflict` when the object no longer holds what that step left: someone
    /// else changed it after, and their edit must not be overwritten. The history is only moved
    /// when the returned operation is applied and accepted, so a refusal changes nothing.
    pub fn undo(&mut self, session: &Session, head: u64, actor: &str, op_id: &str) -> Result<OpEnvelope, Rejection> {
        let step = self.history.of(actor).peek_undo().cloned().ok_or_else(|| Rejection::invalid("nothing to undo", Some(head)))?;
        let doc = session.doc().map_err(|_| Rejection::invalid("no document open", Some(head)))?.doc.clone();
        let current =
            self.capture_snapshot(&doc, &step.before).ok_or_else(|| Rejection::undo_conflict("the object of this step is gone", Some(head)))?;
        if current != step.after {
            return Err(Rejection::undo_conflict("someone else changed this object after the step", Some(head)));
        }
        let (slide, shape, payload) = step.restoring_payload();
        Ok(OpEnvelope {
            op_id: op_id.to_string(),
            doc: String::new(),
            base_version: head,
            kind: OpKind::Command,
            target: Target { slide: Some(slide), shape: Some(shape), ..Target::default() },
            payload,
            history: Some(HistoryAction::Undo { op_id: step.op_id.clone() }),
        })
    }

    /// Build the operation that re-applies `actor`'s last undone step.
    pub fn redo(&mut self, session: &Session, head: u64, actor: &str, op_id: &str) -> Result<OpEnvelope, Rejection> {
        let step = self.history.of(actor).peek_redo().cloned().ok_or_else(|| Rejection::invalid("nothing to redo", Some(head)))?;
        let doc = session.doc().map_err(|_| Rejection::invalid("no document open", Some(head)))?.doc.clone();
        let current =
            self.capture_snapshot(&doc, &step.after).ok_or_else(|| Rejection::undo_conflict("the object of this step is gone", Some(head)))?;
        if current != step.before {
            return Err(Rejection::undo_conflict("someone else changed this object after the undo", Some(head)));
        }
        let payload = match &step.after.image {
            Image::Xfrm { x, y, w, h } => Payload::ShapeSetBounds { x: *x, y: *y, w: *w, h: *h },
            Image::Text(text) => Payload::TextSet { text: text.clone(), cell: step.after.cell },
        };
        Ok(OpEnvelope {
            op_id: op_id.to_string(),
            doc: String::new(),
            base_version: head,
            kind: OpKind::Command,
            target: Target { slide: Some(step.after.slide.clone()), shape: Some(step.after.shape.clone()), ..Target::default() },
            payload,
            history: Some(HistoryAction::Redo { op_id: step.op_id.clone() }),
        })
    }

    /// The image an operation replaces, when it is a merge of one object's values.
    fn capture(&self, doc: &Presentation, op: &OpEnvelope) -> Option<Snapshot> {
        // Only a merge of one object's values is undoable in v1; a structural step is not.
        let (cell, image) = match &op.payload {
            Payload::ShapeMove { .. } | Payload::ShapeSetBounds { .. } => (None, Image::Xfrm { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }),
            Payload::TextSet { cell, .. } => (*cell, Image::Text(String::new())),
            Payload::TextInsert { .. } | Payload::TextDelete { .. } => (None, Image::Text(String::new())),
            _ => return None,
        };
        let snapshot = Snapshot { slide: op.target.slide.clone()?, shape: op.target.shape.clone()?, cell, image };
        self.capture_snapshot(doc, &snapshot)
    }

    /// Read the current value of one object, if this adapter knows its keys.
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
        };
        Some(Snapshot { slide: at.slide.clone(), shape: at.shape.clone(), cell: at.cell, image })
    }

    /// Resolve the operation and run the engine command it normalises to.
    fn run(&mut self, session: &mut Session, before: &Presentation, head: u64, op: &OpEnvelope) -> Result<(), Rejection> {
        match &op.payload {
            Payload::ShapeMove { x, y } => {
                let (_, shape) = self.resolve_one(before, &op.target, head)?;
                let args = json!({"ids": [shape.0], "x": x, "y": y});
                self.call(session, "shape.move", args, head)
            }
            Payload::ShapeSetBounds { x, y, w, h } => {
                let (_, shape) = self.resolve_one(before, &op.target, head)?;
                let args = json!({"ids": [shape.0], "x": x, "y": y, "w": w, "h": h});
                self.call(session, "shape.setBounds", args, head)
            }
            Payload::TextSet { text, cell } => {
                let (_, shape) = self.resolve_one(before, &op.target, head)?;
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
        | Payload::TextSet { .. }
        | Payload::TextInsert { .. }
        | Payload::TextDelete { .. }
        | Payload::ShapeFill { .. }
        | Payload::ShapeLine { .. }
        | Payload::FormatSet { .. } => OpKind::Command,
        Payload::ShapeInsert { .. }
        | Payload::PictureInsert { .. }
        | Payload::MediaInsert { .. }
        | Payload::ShapeDelete {}
        | Payload::Reorder { .. }
        | Payload::Group {}
        | Payload::Ungroup {}
        | Payload::SlideInsert { .. }
        | Payload::SlideDelete {}
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
    use deckcraft_collab_proto::{FillSpec, FormatCmd, LineSpec, TextCursor};

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
}
