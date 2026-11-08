//! The client side of one document: normalise local commands into operations, send them, and
//! apply the authoritative commands the server broadcasts.
//!
//! The client never guesses the result of an operation: a local edit changes the document only
//! when `applied` comes back. That keeps every replica on the server's order and makes a refused
//! operation a no-op instead of a rollback (RFC §10; optimistic previews are a later slice).
//!
//! Keys are derived, not transferred: both the server's adapter and this one walk the same
//! document in the same order with the same `fileId` prefix, so they issue the same session keys
//! (ADR 0002). A version gap is repaired by `resync`, not by guessing a key.

use std::fmt;

use deckcraft_collab_proto::{
    Applied, Change, ClientInfo, ClientMsg, ConfirmedCommand, OpEnvelope, OpKind, Payload, ServerMsg, ShapeKey, SlideKey, Target,
};
use deckcraft_engine::{DocState, Session, Target as EngineTarget};
use deckcraft_model::{Presentation, Shape, ShapeId, SlideId, find_shape, parent_of};
use serde_json::Value;

use crate::apply::{Adapter, class_of};
use crate::client::{ClientSession, Effect};

/// Why a local command could not be turned into an operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalError {
    /// A viewer may not change the document.
    ReadOnly,
    /// Too many operations are waiting for an answer.
    Busy,
    /// No presentation open (or not a slide view).
    NoDocument,
    /// This command has no normalised form in collaboration v1.
    Unsupported(String),
    BadParams(String),
}

impl fmt::Display for LocalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LocalError::ReadOnly => f.write_str("view only: you cannot change the presentation"),
            LocalError::Busy => f.write_str("too many operations are waiting for the server"),
            LocalError::NoDocument => f.write_str("no slide is open"),
            LocalError::Unsupported(cmd) => write!(f, "`{cmd}` is not available in a shared session yet"),
            LocalError::BadParams(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for LocalError {}

impl From<crate::SubmitError> for LocalError {
    fn from(e: crate::SubmitError) -> Self {
        match e {
            crate::SubmitError::ReadOnly => LocalError::ReadOnly,
            crate::SubmitError::Busy => LocalError::Busy,
        }
    }
}

/// One client of a shared document: the state machine plus the adapter that resolves keys and
/// applies the server's confirmed commands.
pub struct CollabDoc {
    file_id: String,
    client: ClientSession,
    adapter: Adapter,
    /// Version of the local document.
    version: u64,
    /// The last local command that could not be applied, for the status bar.
    last_apply_error: Option<String>,
    /// Undo/redo requests waiting for a live link (they are not operations until the server builds
    /// the inverse).
    control: std::collections::VecDeque<ClientMsg>,
}

impl CollabDoc {
    pub fn new(file_id: impl Into<String>) -> Self {
        let file_id = file_id.into();
        let adapter = Adapter::new(format!("{file_id}-"));
        CollabDoc {
            client: ClientSession::new(file_id.clone()),
            file_id,
            adapter,
            version: 0,
            last_apply_error: None,
            control: std::collections::VecDeque::new(),
        }
    }

    /// Start a connection and issue keys for the document the host opened. The caller sends the
    /// returned `hello`; the answer is fed back through [`Self::on_msg`].
    pub fn connect(&mut self, session: &Session, jwt: impl Into<String>, client: ClientInfo) -> ClientMsg {
        if let Some(st) = session.active() {
            self.adapter.join(&st.doc);
        }
        self.client.connect(jwt, client)
    }

    pub fn next_outgoing(&mut self) -> Option<ClientMsg> {
        // Control messages wait for the handshake, then go before operations.
        if self.client.link() == crate::Link::Live
            && matches!(self.client.role(), Some(role) if role.may_write())
            && let Some(msg) = self.control.pop_front()
        {
            return Some(msg);
        }
        self.client.next_outgoing()
    }

    /// Ask the server to take back this actor's latest step.
    pub fn undo(&mut self) -> Result<(), LocalError> {
        if self.client.role().is_some_and(|role| !role.may_write()) {
            return Err(LocalError::ReadOnly);
        }
        self.control.push_back(ClientMsg::Undo);
        Ok(())
    }

    /// Ask the server to re-apply the step the latest undo took back.
    pub fn redo(&mut self) -> Result<(), LocalError> {
        if self.client.role().is_some_and(|role| !role.may_write()) {
            return Err(LocalError::ReadOnly);
        }
        self.control.push_back(ClientMsg::Redo);
        Ok(())
    }

    /// The socket dropped: keep unacknowledged operations for the resend and wait for a new
    /// handshake. The caller starts it with [`Self::connect`].
    pub fn disconnected(&mut self) {
        self.client.disconnected();
    }

    pub fn client(&self) -> &ClientSession {
        &self.client
    }

    pub fn keys(&self) -> &crate::KeyMap {
        self.adapter.keys()
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn last_apply_error(&self) -> Option<&str> {
        self.last_apply_error.as_deref()
    }

    /// Normalise a local UI command and queue its operations. Nothing is applied locally: the
    /// document changes when the server's `applied` comes back.
    pub fn submit(&mut self, session: &Session, cmd: &str, params: &Value, op_id: &str) -> Result<usize, LocalError> {
        let st = session.active().ok_or(LocalError::NoDocument)?;
        let ops = normalize(self.adapter.keys(), st, cmd, params, op_id, &self.file_id)?;
        for op in &ops {
            self.client.submit(op.clone())?;
        }
        Ok(ops.len())
    }

    /// Apply one authoritative command and move the local version. `false` when the command was
    /// for a version that is not the next one (the caller waits for `resync`).
    fn apply_command(&mut self, session: &mut Session, version: u64, actor: &str, command: &ConfirmedCommand) -> bool {
        if version != self.version + 1 {
            return false;
        }
        let op = OpEnvelope {
            op_id: String::new(),
            doc: self.file_id.clone(),
            base_version: self.version,
            kind: class_of(&command.payload),
            target: command.target.clone(),
            payload: command.payload.clone(),
            history: None,
        };
        match self.adapter.apply_confirmed(session, self.version, actor, &op) {
            Ok(_) => {
                self.version = version;
                self.last_apply_error = None;
                true
            }
            Err(e) => {
                self.last_apply_error = Some(e.message);
                false
            }
        }
    }

    /// Handle one server message: move the client state machine, apply the confirmed command and
    /// replay a snapshot tail. Presence and commit results are left to the caller.
    pub fn on_msg(&mut self, session: &mut Session, msg: ServerMsg) -> Effect {
        let echo = msg.clone();
        let effect = self.client.on_msg(msg);
        match echo {
            ServerMsg::Welcome(welcome) => {
                if welcome.version > self.version {
                    // The host-opened bytes are older than the session: the next resync brings
                    // the base and tail. Until then the local replica is behind and must not
                    // apply commands out of order.
                    self.last_apply_error = None;
                }
            }
            ServerMsg::Applied(applied) => self.apply_broadcast(session, &applied),
            ServerMsg::Snapshot(snapshot) => {
                for applied in &snapshot.tail {
                    self.apply_broadcast(session, applied);
                }
                if snapshot.snapshot.mode == deckcraft_collab_proto::SnapshotMode::Base && snapshot.tail.is_empty() {
                    // A base-only snapshot replaces the replica; the transport delivers the
                    // model separately (see `SnapshotMsg::base64` in the RFC).
                    self.version = snapshot.version;
                }
            }
            _ => {}
        }
        effect
    }

    fn apply_broadcast(&mut self, session: &mut Session, applied: &Applied) {
        if let Some(command) = &applied.command {
            let _ = self.apply_command(session, applied.version, &applied.actor_id, command);
        } else if applied.version == self.version + 1 {
            self.version = applied.version;
        }
    }
}

/// Normalise one local UI command into wire operations. Values are final (absolute positions,
/// whole text bodies, explicit targets); commands with no v1 form are refused rather than sent
/// in a shape the server cannot reproduce.
pub fn normalize(keys: &crate::KeyMap, st: &DocState, cmd: &str, p: &Value, op_id: &str, doc: &str) -> Result<Vec<OpEnvelope>, LocalError> {
    if st.selection.target != EngineTarget::Slides {
        return Err(LocalError::Unsupported(cmd.into()));
    }
    let slide = st.doc.slides.get(st.selection.slide).ok_or(LocalError::NoDocument)?;
    let make = |target: Target, payload: Payload| OpEnvelope {
        op_id: op_id.to_string(),
        doc: doc.to_string(),
        base_version: 0,
        kind: class_of(&payload),
        target,
        payload,
        history: None,
    };
    let slide_key = keys.slide_key(slide.id).cloned();
    match cmd {
        "shape.move" => {
            let ids = shape_ids(p, st)?;
            let (dx, dy) = (p.get("dx").and_then(Value::as_f64), p.get("dy").and_then(Value::as_f64));
            let (ax, ay) = (p.get("x").and_then(Value::as_f64), p.get("y").and_then(Value::as_f64));
            if dx.is_none() && dy.is_none() && ax.is_none() && ay.is_none() {
                return Err(LocalError::BadParams("shape.move needs dx/dy or x/y".into()));
            }
            let mut out = Vec::new();
            for id in ids {
                let shape = find_shape(&slide.shapes, id).ok_or_else(|| LocalError::BadParams("no such shape".into()))?;
                let xfrm = current_xfrm(&st.doc, st, shape);
                let x = ax.unwrap_or(xfrm.x + dx.unwrap_or(0.0));
                let y = ay.unwrap_or(xfrm.y + dy.unwrap_or(0.0));
                out.push(make(
                    Target { slide: slide_key.clone(), shape: Some(shape_key(keys, slide.id, id)?), ..Target::default() },
                    Payload::ShapeMove { x, y },
                ));
            }
            Ok(out)
        }
        "shape.setBounds" => {
            let ids = shape_ids(p, st)?;
            let mut out = Vec::new();
            for id in ids {
                let shape = find_shape(&slide.shapes, id).ok_or_else(|| LocalError::BadParams("no such shape".into()))?;
                let xfrm = current_xfrm(&st.doc, st, shape);
                let x = p.get("x").and_then(Value::as_f64).unwrap_or(xfrm.x);
                let y = p.get("y").and_then(Value::as_f64).unwrap_or(xfrm.y);
                let w = p.get("w").and_then(Value::as_f64).unwrap_or(xfrm.w);
                let h = p.get("h").and_then(Value::as_f64).unwrap_or(xfrm.h);
                out.push(make(
                    Target { slide: slide_key.clone(), shape: Some(shape_key(keys, slide.id, id)?), ..Target::default() },
                    Payload::ShapeSetBounds { x, y, w, h },
                ));
            }
            Ok(out)
        }
        "text.set" => {
            let id = p
                .get("id")
                .and_then(Value::as_u64)
                .map(|v| ShapeId(v as u32))
                .or_else(|| st.selection.text.as_ref().map(|t| t.shape))
                .or_else(|| st.selection.shapes.first().copied())
                .ok_or_else(|| LocalError::BadParams("text.set needs a shape".into()))?;
            let text = p.get("text").and_then(Value::as_str).ok_or_else(|| LocalError::BadParams("text.set needs `text`".into()))?;
            let cell = p
                .get("cell")
                .and_then(|c| c.as_array())
                .and_then(|a| Some((u32::try_from(a.first()?.as_u64()?).ok()?, u32::try_from(a.get(1)?.as_u64()?).ok()?)));
            Ok(vec![make(
                Target { slide: slide_key.clone(), shape: Some(shape_key(keys, slide.id, id)?), ..Target::default() },
                Payload::TextSet { text: text.to_string(), cell },
            )])
        }
        "edit.delete" => {
            if p.get("scope").and_then(Value::as_str) == Some("slides") {
                return slide_delete_ops(keys, st, p, make);
            }
            let ids = shape_ids(p, st)?;
            if ids.is_empty() {
                return Err(LocalError::BadParams("nothing selected".into()));
            }
            let shapes: Vec<ShapeKey> = ids.into_iter().map(|id| shape_key(keys, slide.id, id)).collect::<Result<_, _>>()?;
            Ok(vec![make(Target { slide: slide_key, shapes, ..Target::default() }, Payload::ShapeDelete {})])
        }
        "slide.new" => {
            if p.get("title").is_some() || p.get("body").is_some() {
                return Err(LocalError::Unsupported("slide.new with title/body".into()));
            }
            let n = st.doc.slides.len();
            let at = p.get("at").and_then(Value::as_u64).map(|v| v as usize).unwrap_or((st.selection.slide + 1).min(n)).min(n);
            let before = st.doc.slides.get(at).and_then(|s| keys.slide_key(s.id).cloned());
            let layout = p.get("layout").and_then(Value::as_str).map(str::to_string);
            Ok(vec![make(Target::default(), Payload::SlideInsert { before, layout })])
        }
        "slide.delete" => slide_delete_ops(keys, st, p, make),
        "slide.move" => {
            let moved: Vec<SlideId> = if let Some(ids) = p.get("ids").and_then(Value::as_array) {
                ids.iter().filter_map(Value::as_u64).map(|v| SlideId(v as u32)).collect()
            } else if let Some(from) = p.get("from").and_then(Value::as_u64) {
                st.doc.slides.get(from as usize).map(|s| s.id).into_iter().collect()
            } else {
                st.selection.slides.clone()
            };
            let to = p.get("to").and_then(Value::as_u64).ok_or_else(|| LocalError::BadParams("slide.move needs `to`".into()))? as usize;
            if moved.len() != 1 {
                return Err(LocalError::Unsupported("moving several slides at once".into()));
            }
            let id = moved[0];
            let rest: Vec<&deckcraft_model::Slide> = st.doc.slides.iter().map(|s| s.as_ref()).filter(|s| s.id != id).collect();
            let before = rest.get(to).and_then(|s| keys.slide_key(s.id).cloned());
            Ok(vec![make(Target { slide: keys.slide_key(id).cloned(), ..Target::default() }, Payload::SlideMove { before })])
        }
        "arrange.group" | "arrange.ungroup" => {
            let ids = shape_ids(p, st)?;
            if cmd == "arrange.group" {
                if ids.len() < 2 {
                    return Err(LocalError::BadParams("a group needs two or more shapes".into()));
                }
                let shapes: Vec<ShapeKey> = ids.into_iter().map(|id| shape_key(keys, slide.id, id)).collect::<Result<_, _>>()?;
                Ok(vec![make(Target { slide: slide_key, shapes, ..Target::default() }, Payload::Group {})])
            } else {
                let id = *ids.first().ok_or_else(|| LocalError::BadParams("ungroup needs a shape".into()))?;
                Ok(vec![make(Target { slide: slide_key, shape: Some(shape_key(keys, slide.id, id)?), ..Target::default() }, Payload::Ungroup {})])
            }
        }
        "arrange.reorder" => {
            let id = p
                .get("id")
                .and_then(Value::as_u64)
                .map(|v| ShapeId(v as u32))
                .or_else(|| st.selection.shapes.first().copied())
                .ok_or_else(|| LocalError::BadParams("arrange.reorder needs `id`".into()))?;
            let index = p.get("index").and_then(Value::as_u64).ok_or_else(|| LocalError::BadParams("arrange.reorder needs `index`".into()))? as usize;
            let siblings = sibling_list(&slide.shapes, id).ok_or_else(|| LocalError::BadParams("no such shape".into()))?;
            let rest: Vec<&Shape> = siblings.iter().filter(|s| s.id != id).collect();
            let before = rest.get(index).map(|s| shape_key(keys, slide.id, s.id)).transpose()?;
            Ok(vec![make(Target { slide: slide_key, shape: Some(shape_key(keys, slide.id, id)?), ..Target::default() }, Payload::Reorder { before })])
        }
        other => Err(LocalError::Unsupported(other.into())),
    }
}

fn slide_delete_ops(
    keys: &crate::KeyMap,
    st: &DocState,
    p: &Value,
    make: impl Fn(Target, Payload) -> OpEnvelope,
) -> Result<Vec<OpEnvelope>, LocalError> {
    let ids: Vec<SlideId> = if let Some(ids) = p.get("ids").and_then(Value::as_array) {
        ids.iter().filter_map(Value::as_u64).map(|v| SlideId(v as u32)).collect()
    } else if let Some(index) = p.get("index").and_then(Value::as_u64) {
        st.doc.slides.get(index as usize).map(|s| s.id).into_iter().collect()
    } else if !st.selection.slides.is_empty() {
        st.selection.slides.clone()
    } else {
        st.doc.slides.get(st.selection.slide).map(|s| s.id).into_iter().collect()
    };
    if ids.is_empty() {
        return Err(LocalError::BadParams("nothing to delete".into()));
    }
    Ok(ids
        .into_iter()
        .filter_map(|id| keys.slide_key(id).map(|key| make(Target { slide: Some(key.clone()), ..Target::default() }, Payload::SlideDelete {})))
        .collect())
}

fn shape_ids(p: &Value, st: &DocState) -> Result<Vec<ShapeId>, LocalError> {
    if let Some(ids) = p.get("ids").and_then(Value::as_array) {
        return Ok(ids.iter().filter_map(Value::as_u64).map(|v| ShapeId(v as u32)).collect());
    }
    if let Some(id) = p.get("id").and_then(Value::as_u64) {
        return Ok(vec![ShapeId(id as u32)]);
    }
    if !st.selection.shapes.is_empty() {
        return Ok(st.selection.shapes.clone());
    }
    Err(LocalError::BadParams("nothing selected".into()))
}

fn shape_key(keys: &crate::KeyMap, slide: SlideId, id: ShapeId) -> Result<ShapeKey, LocalError> {
    keys.shape_key(slide, id).cloned().ok_or(LocalError::NoDocument)
}

fn current_xfrm(doc: &Presentation, st: &DocState, shape: &Shape) -> deckcraft_model::Xfrm {
    deckcraft_engine::cmd::xfrm_of(doc, &st.selection, shape)
}

fn sibling_list(shapes: &[Shape], id: ShapeId) -> Option<&[Shape]> {
    match parent_of(shapes, id) {
        Some(None) | None => Some(shapes),
        Some(Some(group)) => find_shape(shapes, group).map(Shape::children),
    }
}

/// A short, safe operation id for a local command. The transport may replace it with a UUID.
pub fn local_op_id(seed: u64) -> String {
    format!("local-{seed}")
}

/// Only used by the UI to keep a stable shape while a command is in flight.
pub fn is_merge(payload: &Payload) -> bool {
    matches!(payload, Payload::ShapeMove { .. } | Payload::ShapeSetBounds { .. } | Payload::TextSet { .. })
}

/// Description of a change for logs and tests (no content).
pub fn change_summary(change: &Change) -> (&'static str, Option<&SlideKey>, Option<&ShapeKey>) {
    let kind = match change.kind {
        deckcraft_collab_proto::ChangeKind::Inserted => "inserted",
        deckcraft_collab_proto::ChangeKind::Deleted => "deleted",
        deckcraft_collab_proto::ChangeKind::Modified { .. } => "modified",
        deckcraft_collab_proto::ChangeKind::Reordered { .. } => "reordered",
    };
    (kind, Some(&change.slide), change.shape.as_ref())
}

/// The kind of a locally normalised operation, for tests and the status bar.
pub fn kind_of(payload: &Payload) -> OpKind {
    class_of(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_engine::Session;
    use serde_json::json;

    fn deck() -> (Session, String, SlideId, Vec<ShapeId>) {
        let mut s = Session::with_new();
        let title = s.doc().expect("doc").doc.slides[0].id;
        s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("blank");
        s.apply_remote("slide.delete", &json!({"ids": [title.0]})).expect("drop title");
        s.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).expect("rect");
        s.apply_remote("shape.insert", &json!({"preset": "ellipse", "rect": [400, 100, 120, 100]})).expect("ellipse");
        let st = s.doc().expect("doc");
        let slide = st.doc.slides[0].id;
        let shapes = st.doc.slides[0].shapes.iter().map(|x| x.id).collect();
        (s, "d".into(), slide, shapes)
    }

    fn keys(doc: &Presentation) -> crate::KeyMap {
        let mut issuer = crate::KeyIssuer::new("d-");
        let mut map = crate::KeyMap::new();
        map.sync(doc, &mut issuer);
        map
    }

    #[test]
    fn a_relative_move_becomes_an_absolute_operation() {
        let (s, doc, slide, shapes) = deck();
        let km = keys(s.doc().expect("doc").doc.as_ref());
        let p = json!({"ids": [shapes[0].0], "dx": 25.0, "dy": -5.0});
        let ops = normalize(&km, s.doc().expect("doc"), "shape.move", &p, "op", &doc).expect("normalise");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].payload, Payload::ShapeMove { x: 125.0, y: 95.0 });
        assert_eq!(ops[0].target.slide.as_ref().and_then(|k| km.slide_id(k)), Some(slide));
        assert_eq!(ops[0].kind, OpKind::Command);
    }

    #[test]
    fn a_structural_command_is_marked_structural_and_ignores_extra_slide_text() {
        let (s, doc, _, _) = deck();
        let km = keys(s.doc().expect("doc").doc.as_ref());
        let ops = normalize(&km, s.doc().expect("doc"), "slide.new", &json!({"layout": "blank", "at": 0}), "op", &doc).expect("normalise");
        assert_eq!(ops[0].kind, OpKind::Structural);
        match &ops[0].payload {
            Payload::SlideInsert { before, layout } => {
                assert!(before.is_some(), "a slide inserted at 0 goes before the first one");
                assert_eq!(layout.as_deref(), Some("blank"));
            }
            other => panic!("{other:?}"),
        }
        let refused = normalize(&km, s.doc().expect("doc"), "slide.new", &json!({"title": "x"}), "op", &doc);
        assert!(matches!(refused, Err(LocalError::Unsupported(_))));
    }

    #[test]
    fn deleting_the_selected_slides_is_one_operation_each() {
        let (mut s, doc, _, _) = deck();
        s.execute("slide.new", &json!({"layout": "blank"})).expect("second");
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());
        let ops = normalize(&km, st, "edit.delete", &json!({"scope": "slides"}), "op", &doc).expect("normalise");
        assert_eq!(ops.len(), 1, "the current slide only");
        assert!(matches!(ops[0].payload, Payload::SlideDelete {}));
        assert_eq!(ops[0].kind, OpKind::Structural);
    }

    #[test]
    fn reorder_turns_an_index_into_an_anchor() {
        let (s, doc, _, shapes) = deck();
        let km = keys(s.doc().expect("doc").doc.as_ref());
        let ops = normalize(&km, s.doc().expect("doc"), "arrange.reorder", &json!({"id": shapes[1].0, "index": 0}), "op", &doc).expect("normalise");
        match &ops[0].payload {
            Payload::Reorder { before } => assert_eq!(before.as_ref(), km.shape_key(s.doc().unwrap().doc.slides[0].id, shapes[0]).cloned().as_ref()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn commands_outside_the_v1_set_are_refused_by_name() {
        let (s, doc, _, _) = deck();
        let km = keys(s.doc().expect("doc").doc.as_ref());
        let e = normalize(&km, s.doc().expect("doc"), "format.bold", &json!({}), "op", &doc).expect_err("refused");
        assert_eq!(e, LocalError::Unsupported("format.bold".into()));
    }

    #[test]
    fn a_client_applies_the_broadcast_command_and_moves_its_version() {
        let (mut s, _doc, slide, shapes) = deck();
        let mut client = CollabDoc::new("d");
        client.connect(&s, "jwt", ClientInfo { name: "test".into(), ver: "1".into() });
        let welcome = ServerMsg::Welcome(deckcraft_collab_proto::Welcome {
            v: deckcraft_collab_proto::PROTO_VERSION,
            session_id: "s".into(),
            actor_id: "alice".into(),
            role: deckcraft_collab_proto::Role::Editor,
            lease: None,
            version: 0,
            snapshot: deckcraft_collab_proto::SnapshotRef { mode: deckcraft_collab_proto::SnapshotMode::BaseAndTail, cursor: "tail@0".into() },
            capabilities: deckcraft_collab_proto::Capabilities {
                kinds: vec![OpKind::Command, OpKind::Structural],
                limits: deckcraft_collab_proto::Limits::default(),
                commands: vec![],
            },
        });
        client.on_msg(&mut s, welcome);
        let key = client.keys().shape_key(slide, shapes[0]).expect("key").clone();
        let applied = Applied {
            op_id: "op".into(),
            version: 1,
            actor_id: "bob".into(),
            command: Some(ConfirmedCommand {
                cmd: "shape.move".into(),
                target: Target { slide: Some(client.keys().slide_key(slide).expect("slide").clone()), shape: Some(key), ..Target::default() },
                payload: Payload::ShapeMove { x: 300.0, y: 100.0 },
            }),
            delta: Default::default(),
        };
        client.on_msg(&mut s, ServerMsg::Applied(applied));
        assert_eq!(client.version(), 1);
        let moved = find_shape(&s.doc().expect("doc").doc.slides[0].shapes, shapes[0]).expect("shape");
        assert_eq!(moved.xfrm.expect("xfrm").x, 300.0);
        assert!(client.last_apply_error().is_none());
    }
}
