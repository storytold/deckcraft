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
use std::sync::Arc;

use base64::Engine as _;
use deckcraft_collab_proto::{
    Applied, Change, ClientInfo, ClientMsg, ConfirmedCommand, OpEnvelope, OpKind, Payload, ServerMsg, ShapeKey, SlideKey, Target,
};
use deckcraft_engine::{DocState, Session, Target as EngineTarget};
use deckcraft_model::{Presentation, Shape, ShapeId, SlideId, find_shape, parent_of};
use serde_json::Value;

use crate::apply::{Adapter, class_of, position_offset};
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
    /// Where a replica gets picture bytes it does not have yet (`GET /media/{sha256}`). The
    /// transport owns the connection, so the host supplies this callback; without it a picture
    /// inserted by someone else cannot be applied and the status bar says so.
    media_fetcher: Option<Arc<dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync>>,
    /// Pictures this replica should upload before its `picture.insert` reaches the server.
    uploads: std::collections::VecDeque<(String, Arc<Vec<u8>>)>,
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
            media_fetcher: None,
            uploads: std::collections::VecDeque::new(),
        }
    }

    /// Picture bytes waiting to be uploaded before their operation is sent. The host drains this
    /// every frame and calls [`crate::Transport::put_media`]; the operation is already queued, so
    /// an upload that fails ends in a server `rejected`, never in a silent divergence.
    pub fn take_uploads(&mut self) -> Vec<(String, Arc<Vec<u8>>)> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        while let Some((sha, bytes)) = self.uploads.pop_front() {
            if seen.insert(sha.clone()) {
                out.push((sha, bytes));
            }
        }
        out
    }

    /// Supply the media fetch for a client that is not the picture's author. Called with the
    /// digest; returns the bytes or `None` when they cannot be fetched.
    pub fn set_media_fetcher(&mut self, fetch: Arc<dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync>) {
        self.media_fetcher = Some(fetch);
    }

    /// Register picture bytes this replica has (the author uploads them through `submit`).
    pub fn put_media(&mut self, name: &str, bytes: Vec<u8>) -> Result<String, LocalError> {
        self.adapter.media_mut().put(name, bytes).map_err(|e| LocalError::BadParams(e.message))
    }

    pub fn has_media(&self, sha: &str) -> bool {
        self.adapter.media().contains(sha)
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
        // A picture travels in two pieces: the bytes to the media channel, the digest on the
        // operation. The author always has the bytes, so the confirmed command can run here too.
        let mut wire;
        let params = if cmd == "insert.picture" {
            let (name, bytes) = picture_bytes(params)?;
            let sha = self.adapter.media_mut().put(&name, bytes).map_err(|e| LocalError::BadParams(e.message))?;
            if let Some(stored) = self.adapter.media().get(&sha) {
                self.uploads.push_back((sha.clone(), stored));
            }
            wire = params.clone();
            if let Some(object) = wire.as_object_mut() {
                object.remove("data");
                object.remove("path");
            }
            wire["sha256"] = Value::String(sha);
            wire["name"] = Value::String(name);
            &wire
        } else {
            params
        };
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
        if let Payload::PictureInsert { sha256, .. } = &command.payload
            && !self.adapter.media().contains(sha256)
            && let Some(fetch) = self.media_fetcher.clone()
            && let Some(bytes) = fetch(sha256)
            && let Err(e) = self.adapter.media_mut().insert(sha256, bytes)
        {
            self.last_apply_error = Some(e.message);
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
            if let Some(ts) = st.selection.text.as_ref() {
                if ts.notes || ts.cell.is_some() {
                    return Err(LocalError::Unsupported("editing notes or a table cell".into()));
                }
                let shape = find_shape(&slide.shapes, ts.shape).ok_or_else(|| LocalError::BadParams("no such shape".into()))?;
                let (from, to) = (position_offset(shape.text.as_ref(), ts.ordered().0), position_offset(shape.text.as_ref(), ts.ordered().1));
                if to <= from {
                    return Err(LocalError::BadParams("nothing selected".into()));
                }
                return Ok(vec![make(
                    Target { slide: slide_key, shape: Some(shape_key(keys, slide.id, ts.shape)?), ..Target::default() },
                    Payload::TextDelete { from: from as u32, to: to as u32 },
                )]);
            }
            let ids = shape_ids(p, st)?;
            if ids.is_empty() {
                return Err(LocalError::BadParams("nothing selected".into()));
            }
            let shapes: Vec<ShapeKey> = ids.into_iter().map(|id| shape_key(keys, slide.id, id)).collect::<Result<_, _>>()?;
            Ok(vec![make(Target { slide: slide_key, shapes, ..Target::default() }, Payload::ShapeDelete {})])
        }
        "text.insert" => {
            let ts = st.selection.text.as_ref().ok_or_else(|| LocalError::BadParams("text.insert needs a caret".into()))?;
            if ts.notes || ts.cell.is_some() {
                return Err(LocalError::Unsupported("typing in notes or a table cell".into()));
            }
            let text = p.get("text").and_then(Value::as_str).ok_or_else(|| LocalError::BadParams("text.insert needs `text`".into()))?;
            let shape = find_shape(&slide.shapes, ts.shape).ok_or_else(|| LocalError::BadParams("no such shape".into()))?;
            let at = position_offset(shape.text.as_ref(), ts.caret);
            Ok(vec![make(
                Target { slide: slide_key, shape: Some(shape_key(keys, slide.id, ts.shape)?), ..Target::default() },
                Payload::TextInsert { at: at as u32, text: text.to_string() },
            )])
        }
        "text.delete" => {
            let ts = st.selection.text.as_ref().ok_or_else(|| LocalError::BadParams("text.delete needs a text selection".into()))?;
            if ts.notes || ts.cell.is_some() {
                return Err(LocalError::Unsupported("editing notes or a table cell".into()));
            }
            let shape = find_shape(&slide.shapes, ts.shape).ok_or_else(|| LocalError::BadParams("no such shape".into()))?;
            let (from, to) = (position_offset(shape.text.as_ref(), ts.ordered().0), position_offset(shape.text.as_ref(), ts.ordered().1));
            if to <= from {
                return Err(LocalError::BadParams("nothing selected".into()));
            }
            Ok(vec![make(
                Target { slide: slide_key, shape: Some(shape_key(keys, slide.id, ts.shape)?), ..Target::default() },
                Payload::TextDelete { from: from as u32, to: to as u32 },
            )])
        }
        "shape.insert" => {
            let preset = p.get("preset").and_then(Value::as_str).ok_or_else(|| LocalError::BadParams("shape.insert needs `preset`".into()))?;
            let rect = p
                .get("rect")
                .and_then(Value::as_array)
                .filter(|a| a.len() == 4)
                .ok_or_else(|| LocalError::BadParams("shape.insert needs `rect: [x,y,w,h]`".into()))?;
            let mut r = [0.0; 4];
            for (i, v) in rect.iter().enumerate() {
                r[i] = v.as_f64().ok_or_else(|| LocalError::BadParams("shape.insert rect values must be numbers".into()))?;
            }
            // Anything else (flip, placeholder, custom geometry) has no v1 form: refuse it rather
            // than silently dropping the difference on another replica.
            for key in p.as_object().map(|o| o.keys()).into_iter().flatten() {
                if !matches!(key.as_str(), "preset" | "rect" | "text") {
                    return Err(LocalError::Unsupported(format!("shape.insert {key}")));
                }
            }
            let text = p.get("text").and_then(Value::as_str).map(str::to_string);
            Ok(vec![make(Target { slide: slide_key, ..Target::default() }, Payload::ShapeInsert { preset: preset.to_string(), rect: r, text })])
        }
        "insert.picture" => {
            let sha = p.get("sha256").and_then(Value::as_str).ok_or_else(|| LocalError::BadParams("insert.picture needs a media digest".into()))?;
            if !crate::media::valid_sha(sha) {
                return Err(LocalError::BadParams("insert.picture: the media digest is not a sha256".into()));
            }
            let name = p.get("name").and_then(Value::as_str).unwrap_or("media").to_string();
            let rect = match p.get("rect") {
                None => None,
                Some(value) => {
                    let array = value
                        .as_array()
                        .filter(|a| a.len() == 4)
                        .ok_or_else(|| LocalError::BadParams("insert.picture rect must be [x,y,w,h]".into()))?;
                    let mut r = [0.0; 4];
                    for (i, v) in array.iter().enumerate() {
                        r[i] = v.as_f64().ok_or_else(|| LocalError::BadParams("insert.picture rect values must be numbers".into()))?;
                    }
                    Some(r)
                }
            };
            for key in p.as_object().map(|o| o.keys()).into_iter().flatten() {
                if !matches!(key.as_str(), "sha256" | "name" | "rect") {
                    return Err(LocalError::Unsupported(format!("insert.picture {key}")));
                }
            }
            Ok(vec![make(Target { slide: slide_key, ..Target::default() }, Payload::PictureInsert { sha256: sha.to_string(), name, rect })])
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

/// Bytes for `insert.picture`: base64 `data`, or a local `path` on desktop. The web build has
/// no file system, so the host passes `data`.
fn picture_bytes(p: &Value) -> Result<(String, Vec<u8>), LocalError> {
    let name = p.get("name").and_then(Value::as_str).unwrap_or("media").to_string();
    if let Some(data) = p.get("data").and_then(Value::as_str) {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| LocalError::BadParams("insert.picture: `data` is not base64".into()))?;
        return Ok((name, bytes));
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = p.get("path").and_then(Value::as_str) {
        let bytes = std::fs::read(path).map_err(|e| LocalError::BadParams(format!("{path}: {e}")))?;
        return Ok((name, bytes));
    }
    Err(LocalError::BadParams("insert.picture needs `data` or `path`".into()))
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

    /// 8×8 red PNG, enough for `insert.picture` to decode.
    fn png() -> Vec<u8> {
        vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0, 8, 8, 2, 0, 0, 0, 75, 109, 41, 220, 0, 0, 0, 17, 73,
            68, 65, 84, 120, 156, 99, 184, 163, 161, 129, 21, 49, 12, 45, 9, 0, 123, 123, 75, 1, 251, 162, 81, 240, 0, 0, 0, 0, 73, 69, 78, 68, 174,
            66, 96, 130,
        ]
    }

    fn welcome(version: u64) -> ServerMsg {
        ServerMsg::Welcome(deckcraft_collab_proto::Welcome {
            v: deckcraft_collab_proto::PROTO_VERSION,
            session_id: "s".into(),
            actor_id: "alice".into(),
            role: deckcraft_collab_proto::Role::Editor,
            lease: None,
            version,
            snapshot: deckcraft_collab_proto::SnapshotRef {
                mode: deckcraft_collab_proto::SnapshotMode::BaseAndTail,
                cursor: format!("tail@{version}"),
            },
            capabilities: deckcraft_collab_proto::Capabilities {
                kinds: vec![OpKind::Command, OpKind::Structural],
                limits: deckcraft_collab_proto::Limits::default(),
                commands: vec![],
            },
        })
    }

    #[test]
    fn a_picture_command_uploads_bytes_and_sends_only_a_reference() {
        let (mut s, _doc, _slide, _shapes) = deck();
        let mut client = CollabDoc::new("d");
        client.connect(&s, "jwt", ClientInfo { name: "test".into(), ver: "1".into() });
        client.on_msg(&mut s, welcome(0));
        let bytes = png();
        let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let sent = client.submit(&s, "insert.picture", &json!({"name": "red.png", "data": data}), "op-1").expect("submit");
        assert_eq!(sent, 1);
        let Some(ClientMsg::Op(op)) = client.next_outgoing() else { panic!("expected an operation") };
        match &op.payload {
            Payload::PictureInsert { sha256, name, rect } => {
                assert_eq!(name, "red.png");
                assert!(rect.is_none());
                assert_eq!(sha256, &crate::media::sha256_hex(&bytes), "the operation names the bytes by their digest");
                assert!(!serde_json::to_string(&op.payload).expect("json").contains("data"), "no bytes on the wire");
            }
            other => panic!("{other:?}"),
        }
        assert!(client.has_media(&crate::media::sha256_hex(&bytes)));
        assert_eq!(op.kind, OpKind::Structural);
    }

    #[test]
    fn a_remote_picture_is_fetched_before_it_is_applied() {
        let (mut s, _doc, slide, _shapes) = deck();
        let mut client = CollabDoc::new("d");
        client.connect(&s, "jwt", ClientInfo { name: "test".into(), ver: "1".into() });
        client.on_msg(&mut s, welcome(0));
        let bytes = png();
        let sha = crate::media::sha256_hex(&bytes);
        let wanted = sha.clone();
        let served = bytes.clone();
        client.set_media_fetcher(std::sync::Arc::new(move |asked: &str| (asked == wanted).then(|| served.clone())));
        let slide_key = client.keys().slide_key(slide).expect("slide").clone();
        let applied = Applied {
            op_id: "op".into(),
            version: 1,
            actor_id: "bob".into(),
            command: Some(ConfirmedCommand {
                cmd: "insert.picture".into(),
                target: Target { slide: Some(slide_key), ..Target::default() },
                payload: Payload::PictureInsert { sha256: sha.clone(), name: "red.png".into(), rect: Some([50.0, 50.0, 60.0, 60.0]) },
            }),
            delta: Default::default(),
        };
        client.on_msg(&mut s, ServerMsg::Applied(applied));
        assert_eq!(client.version(), 1, "the fetcher supplied the bytes and the command ran");
        assert!(client.has_media(&sha));
        assert!(client.last_apply_error().is_none());
        let doc = s.doc().expect("doc").doc.clone();
        let picture = doc.slides[0].shapes.iter().find(|s| matches!(s.kind, deckcraft_model::ShapeKind::Picture { .. })).expect("picture");
        assert_eq!(picture.xfrm.expect("xfrm").x, 50.0);
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
