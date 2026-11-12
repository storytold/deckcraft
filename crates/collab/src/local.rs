//! The client side of one document: normalise local commands into operations, send them, and
//! apply the authoritative commands the server broadcasts.
//!
//! The client never guesses the result of an operation: a local edit changes the document only
//! when `applied` comes back, unless the host has turned optimistic preview on (RFC §3). A preview
//! is one operation applied at submit, marked with a local id, settled by its `applied` and rolled
//! back by a refusal; while it is outstanding every later local command waits, so no payload is
//! normalised against an edit only this replica has, and a step from another participant ends the
//! preview with a resync rather than mixing the two orders (RFC §10).
//!
//! Keys are derived, not transferred: both the server's adapter and this one walk the same
//! document in the same order with the same `fileId` prefix, so they issue the same session keys
//! (ADR 0002). A version gap is repaired by `resync`, not by guessing a key.

use std::fmt;
use std::sync::Arc;

use base64::Engine as _;
use deckcraft_collab_proto::{
    Applied, BaseSnapshot, CellFillSpec, Change, ClientInfo, ClientMsg, ConfirmedCommand, FillSpec, FormatAddress, FormatCmd, LineSpec, MediaKind,
    ModelEnvironment, ModelLight, ModelLightKind, ModelScene, OpEnvelope, OpKind, Payload, PicturePlace, ServerMsg, ShapeBounds, ShapeKey, SlideKey,
    SmartArtPictureMedia, SnapshotKeys, TableAxis, Target, TextRange,
};
use deckcraft_engine::{DocState, Session, Target as EngineTarget};
use deckcraft_model::anim::Animation;
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind, Slide, SlideId, Table, find_shape, parent_of};
use serde_json::Value;

use crate::apply::{Adapter, class_of, position_offset};
use crate::client::{ClientSession, Effect, ReadyBase};

/// A model scene as the wire carries it: the model types are the engine's, the wire types are
/// `collab-proto`'s, and the operation carries exactly the final value the engine checked.
fn wire_scene(scene: deckcraft_model::ModelScene) -> ModelScene {
    let kind = match scene.light.kind {
        deckcraft_model::ModelLightKind::Directional => ModelLightKind::Directional,
        deckcraft_model::ModelLightKind::Point => ModelLightKind::Point,
        deckcraft_model::ModelLightKind::Spot => ModelLightKind::Spot,
    };
    ModelScene {
        light: ModelLight { kind, direction: scene.light.direction, intensity: scene.light.intensity, angle: scene.light.angle },
        environment: ModelEnvironment { ambient: scene.environment.ambient, background: scene.environment.background },
    }
}

/// Why a local command could not be turned into an operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalError {
    /// A viewer may not change the document.
    ReadOnly,
    /// Too many operations are waiting for an answer.
    Busy,
    /// The replica is being rebuilt from a base the server is sending in chunks.
    Rebuilding,
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
            LocalError::Rebuilding => f.write_str("the presentation is being rebuilt from the server's copy, try again in a moment"),
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
            crate::SubmitError::Rebuilding => LocalError::Rebuilding,
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
    /// Read-only queries waiting for a live link. A viewer sends them too, so they do not share
    /// the undo/redo control queue.
    queries: std::collections::VecDeque<ClientMsg>,
    /// Answers to this replica's queries, in arrival order.
    query_answers: std::collections::VecDeque<QueryAnswer>,
    /// Whether a local edit is shown before the room confirms it (RFC §3). Off by default, so a
    /// replica that has not opted in behaves exactly as before.
    preview_enabled: bool,
    /// The one operation this replica shows locally and the room has not confirmed yet. While it
    /// is here, later local commands wait in `deferred`: no payload is ever normalised against an
    /// edit only this replica has (RFC §3, "optimistic preview").
    preview: Option<Preview>,
    /// Local commands that arrived while a preview was outstanding, in order. They are normalised
    /// against confirmed state only, after the preview settles or rolls back.
    deferred: std::collections::VecDeque<Deferred>,
}

/// The most local commands held back behind one preview. A user typing faster than the room
/// answers is bounded here rather than growing the queue without limit.
const MAX_DEFERRED: usize = 64;

/// One operation shown locally before its `applied`: the version it was previewed on, the model
/// as it was, and the exact operation, so the authoritative answer can be compared with it.
#[derive(Clone, Debug)]
struct Preview {
    op_id: String,
    before: Arc<Presentation>,
    payload: Payload,
    target: Target,
}

/// A local command held back by an outstanding preview, with its wire parameters already prepared
/// (media digest in place of bytes) and its operation id, which is the caller's per command.
#[derive(Clone, Debug)]
struct Deferred {
    cmd: String,
    params: Value,
    op_id: String,
}

/// One answer to a collaborative query: the authoritative head it ran against and either the
/// engine's value or the server's refusal message. A query changes no document and no version.
#[derive(Clone, Debug, PartialEq)]
pub struct QueryAnswer {
    pub id: String,
    pub version: u64,
    pub result: Option<Value>,
    pub error: Option<String>,
}

/// Whether one operation may be shown locally before the room confirms it.
///
/// Only an operation whose result is a final value the server will not rewrite qualifies: a text
/// insert or delete carries character offsets the room rebases, a table cell address moves under a
/// row/column edit, and a structural step decides ids and places on the server. `SmartArtSet`
/// regenerates child shapes with fresh ids and `FontCreate` installs a face in the session's font
/// database, so neither is a pure document value either.
fn previewable(op: &OpEnvelope) -> bool {
    if op.kind != OpKind::Command || op.history.is_some() {
        return false;
    }
    match &op.payload {
        Payload::ShapeMove { .. }
        | Payload::ShapeSetBounds { .. }
        | Payload::ShapesSetBounds { .. }
        | Payload::ShapeFill { .. }
        | Payload::ShapeLine { .. }
        | Payload::SlideTransition { .. }
        | Payload::ShapeAnimations { .. }
        | Payload::SlideAnimations { .. }
        | Payload::Model3dCamera { .. }
        | Payload::Model3dScene { .. }
        | Payload::ChartSet { .. }
        | Payload::TableStyle { .. }
        | Payload::TableOptions { .. }
        | Payload::TableColumnWidth { .. }
        | Payload::TableRowHeight { .. }
        | Payload::NotesSet { .. }
        | Payload::FontSet { .. }
        | Payload::FontRemove { .. } => true,
        // A whole body (no cell, range or caret) is a final value: formatting changes runs, not
        // the character addresses the room rebases.
        Payload::FormatSet { addresses, .. } => {
            addresses.iter().all(|address| address.cell.is_none() && address.range.is_none() && address.caret.is_none())
        }
        Payload::TextSet { cell, .. } => cell.is_none(),
        _ => false,
    }
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
            queries: std::collections::VecDeque::new(),
            query_answers: std::collections::VecDeque::new(),
            preview_enabled: false,
            preview: None,
            deferred: std::collections::VecDeque::new(),
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
        // Control messages wait for the handshake, then go before operations. A query may come
        // from a viewer, so it is taken before the write-gated undo/redo queue. An undo/redo while
        // a preview is outstanding waits too: the room has not confirmed the edit the user's
        // history would move past.
        if self.client.link() == crate::Link::Live {
            if let Some(msg) = self.queries.pop_front() {
                return Some(msg);
            }
            if self.preview.is_none()
                && matches!(self.client.role(), Some(role) if role.may_write())
                && let Some(msg) = self.control.pop_front()
            {
                return Some(msg);
            }
        }
        self.client.next_outgoing()
    }

    /// Ask the server to run one read-only query against the authoritative document. The answer
    /// arrives through [`Self::on_msg`] and is taken with [`Self::take_query_answer`]. A viewer may
    /// query; the call only queues, so an unknown or mutating command is refused by the answer,
    /// not here.
    pub fn query(&mut self, id: impl Into<String>, cmd: impl Into<String>, target: Target, params: Value) {
        self.queries.push_back(ClientMsg::Query { id: id.into(), cmd: cmd.into(), target, params });
    }

    /// The next answer to a query this replica sent, in arrival order.
    pub fn take_query_answer(&mut self) -> Option<QueryAnswer> {
        self.query_answers.pop_front()
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

    /// Ask the server to commit the room as a new cabinet file version (ADR 0009). One commit in
    /// flight; a repeated Save is the same request. `expected` is the cabinet version the session
    /// was opened from, or the one the last commit was accepted with.
    pub fn commit(&mut self, expected: Option<String>) -> Result<(), crate::CommitError> {
        self.client.commit(expected)
    }

    /// Whether a commit is queued or on the wire.
    pub fn commit_in_flight(&self) -> bool {
        self.client.commit_in_flight()
    }

    /// The cabinet file version the last commit was accepted with.
    pub fn file_version(&self) -> Option<&str> {
        self.client.file_version()
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

    /// Show local edits before the room confirms them (RFC §3). Off by default; a replica with
    /// preview off behaves exactly as before, and the protocol needs no field from such a client.
    pub fn set_preview(&mut self, on: bool) {
        self.preview_enabled = on;
    }

    pub fn preview_enabled(&self) -> bool {
        self.preview_enabled
    }

    /// Whether one local edit is currently shown and not yet confirmed.
    pub fn is_previewing(&self) -> bool {
        self.preview.is_some()
    }

    /// How many local commands are waiting behind that preview.
    pub fn deferred(&self) -> usize {
        self.deferred.len()
    }

    /// Normalise a local UI command and queue its operations.
    ///
    /// With preview off nothing is applied locally: the document changes when the server's
    /// `applied` comes back. With preview on and the room quiet, one operation whose payload the
    /// server will not rewrite is applied at once, marked with its local id, and settled or rolled
    /// back by the answer; every later command waits until then, so it is normalised against
    /// confirmed state and never reads an edit only this replica has.
    pub fn submit(&mut self, session: &mut Session, cmd: &str, params: &Value, op_id: &str) -> Result<usize, LocalError> {
        if self.client.role().is_some_and(|role| !role.may_write()) {
            return Err(LocalError::ReadOnly);
        }
        if session.active().is_none() {
            return Err(LocalError::NoDocument);
        }
        let params = self.prepare_media(cmd, params)?;
        if self.preview_enabled && (self.preview.is_some() || !self.deferred.is_empty()) {
            if self.deferred.len() >= MAX_DEFERRED {
                return Err(LocalError::Busy);
            }
            self.deferred.push_back(Deferred { cmd: cmd.to_string(), params: params.into_owned(), op_id: op_id.to_string() });
            return Ok(0);
        }
        self.submit_prepared(session, cmd, &params, op_id)
    }

    /// Pictures, audio, video, 3-D models and fonts travel in two pieces: the bytes to the media
    /// channel, the digest on the operation. The author always has the bytes, so the confirmed
    /// command can run here too.
    fn prepare_media<'a>(&mut self, cmd: &str, params: &'a Value) -> Result<std::borrow::Cow<'a, Value>, LocalError> {
        let carries_media = matches!(cmd, "insert.picture" | "insert.audio" | "insert.video" | "insert.model3d" | "font.create")
            || (cmd == "smartArt.picture" && (params.get("data").is_some() || params.get("path").is_some()));
        if !carries_media {
            return Ok(std::borrow::Cow::Borrowed(params));
        }
        let (name, bytes) = media_bytes(params, cmd)?;
        let sha = self.adapter.media_mut().put(&name, bytes).map_err(|e| LocalError::BadParams(e.message))?;
        if let Some(stored) = self.adapter.media().get(&sha) {
            self.uploads.push_back((sha.clone(), stored));
        }
        let mut wire = params.clone();
        if let Some(object) = wire.as_object_mut() {
            object.remove("data");
            object.remove("path");
        }
        wire["sha256"] = Value::String(sha);
        wire["name"] = Value::String(name);
        if cmd == "font.create" && params.get("name").is_none() {
            let typeface = params.get("typeface").and_then(Value::as_str).unwrap_or("font");
            wire["name"] = Value::String(format!("{typeface}.ttf"));
        }
        Ok(std::borrow::Cow::Owned(wire))
    }

    /// Normalise prepared parameters, queue the operations, and — when the room is quiet and the
    /// replica agrees with it on the version — show one of them locally.
    fn submit_prepared(&mut self, session: &mut Session, cmd: &str, params: &Value, op_id: &str) -> Result<usize, LocalError> {
        let mut ops = {
            let st = session.active().ok_or(LocalError::NoDocument)?;
            normalize_with_painter(self.adapter.keys(), st, session.anim_painter.as_ref(), cmd, params, op_id, &self.file_id)?
        };
        let preview_now = self.preview_enabled
            && self.preview.is_none()
            && self.deferred.is_empty()
            && self.client.link() == crate::Link::Live
            && self.client.role().is_some_and(|role| role.may_write())
            && self.client.in_flight() == 0
            && self.client.queued() == 0
            && self.version == self.client.version()
            && ops.len() == 1
            && previewable(&ops[0]);
        if preview_now {
            // The wire carries the sender's local id; the room never stores or echoes it, and an
            // operation without one is ordered and deduped the same way (RFC §3).
            ops[0].preview = Some(ops[0].op_id.clone());
        }
        for op in &ops {
            self.client.submit(op.clone())?;
        }
        if preview_now {
            self.start_preview(session, &ops[0]);
        }
        Ok(ops.len())
    }

    /// Apply one queued operation to the local model before the room answers. The model as it was
    /// is kept as an `Arc` snapshot: the engine's edits are copy-on-write, so this is one pointer
    /// copy and restoring it is exact.
    fn start_preview(&mut self, session: &mut Session, op: &OpEnvelope) {
        let Some(before) = session.active().map(|st| st.doc.clone()) else { return };
        match self.adapter.apply_confirmed(session, self.version, "", op) {
            Ok(_) => {
                self.preview = Some(Preview { op_id: op.op_id.clone(), before, payload: op.payload.clone(), target: op.target.clone() });
                self.last_apply_error = None;
            }
            Err(e) => {
                // This replica cannot run it (it may be behind in a way the version does not show),
                // but the room may still accept it: it stays queued and lands on `applied`.
                self.last_apply_error = Some(format!("preview: {}", e.message));
            }
        }
    }

    /// Put the replica back to the model the preview replaced. The revision moves so every cache
    /// in the UI sees a new document, as `edit` does.
    fn restore_before(&self, session: &mut Session, before: Arc<Presentation>) {
        if let Some(st) = session.active_mut() {
            st.doc = before;
            st.revision = st.revision.saturating_add(1);
        }
    }

    /// End the outstanding preview, restoring the model when asked, and release the commands that
    /// waited behind it.
    fn end_preview(&mut self, session: &mut Session, restore: bool) {
        if let Some(preview) = self.preview.take()
            && restore
        {
            self.restore_before(session, preview.before);
        }
    }

    /// Drop what waited behind a preview the room made meaningless (a resync). The commands never
    /// left this replica; normalising them against a rebuilt document could put them somewhere the
    /// user did not mean, so they are reported rather than guessed at.
    fn drop_deferred(&mut self) -> usize {
        let dropped = self.deferred.len();
        self.deferred.clear();
        if dropped > 0 {
            self.last_apply_error =
                Some(format!("the document was resynchronised; {dropped} queued edit{} were not sent", if dropped == 1 { "" } else { "s" }));
        }
        dropped
    }

    /// Normalise and send the commands that waited behind a preview, one at a time: a new preview
    /// pauses the rest, so every payload is normalised against confirmed state.
    fn drain_deferred(&mut self, session: &mut Session) {
        while self.preview.is_none() && self.version == self.client.version() && !self.deferred.is_empty() {
            let Some(next) = self.deferred.pop_front() else { break };
            if let Err(e) = self.submit_prepared(session, &next.cmd, &next.params, &next.op_id) {
                self.last_apply_error = Some(e.to_string());
            }
        }
    }

    /// Apply one authoritative command and move the local version. `false` when the command was
    /// for a version that is not the next one (the caller waits for `resync`).
    fn apply_command(&mut self, session: &mut Session, version: u64, actor: &str, command: &ConfirmedCommand) -> bool {
        if version != self.version + 1 {
            return false;
        }
        if let Some((sha256, kind)) = match &command.payload {
            Payload::PictureInsert { sha256, .. } => Some((sha256, crate::MediaType::Picture)),
            Payload::SmartArtPicture { media: Some(media), .. } => Some((&media.sha256, crate::MediaType::Picture)),
            Payload::FontCreate { sha256, .. } => Some((sha256, crate::MediaType::Font)),
            Payload::MediaInsert { sha256, kind, .. } => Some((
                sha256,
                match kind {
                    MediaKind::Video => crate::MediaType::Video,
                    MediaKind::Audio => crate::MediaType::Audio,
                    MediaKind::Model => crate::MediaType::Model,
                },
            )),
            _ => None,
        } && !self.adapter.media().contains(sha256)
            && let Some(fetch) = self.media_fetcher.clone()
            && let Some(bytes) = fetch(sha256)
            && let Err(e) = self.adapter.media_mut().insert(sha256, bytes)
        {
            self.last_apply_error = Some(format!("{}: {}", kind.noun(), e.message));
        }
        let op = OpEnvelope {
            op_id: String::new(),
            doc: self.file_id.clone(),
            base_version: self.version,
            kind: class_of(&command.payload),
            target: command.target.clone(),
            payload: command.payload.clone(),
            history: None,
            preview: None,
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
    ///
    /// A base the server sends as a chain of chunks (RFC §4.5.1) is assembled by the client state
    /// machine; this host installs it through [`Self::take_ready_base`] once it is whole, so a
    /// snapshot that arrives with `chunks` changes nothing here yet.
    pub fn on_msg(&mut self, session: &mut Session, msg: ServerMsg) -> Effect {
        let echo = msg.clone();
        // A step that is not the one this replica is showing, landing while a preview is
        // outstanding, means the two replicas hold the same operations in different orders. Put
        // the preview back, forget the commands that waited behind it and ask for the tail from
        // the version the replica really has: the alternative is patching the difference by
        // guessing (RFC §3).
        if let ServerMsg::Applied(applied) = &echo
            && self.preview.as_ref().is_some_and(|preview| preview.op_id != applied.op_id)
        {
            self.end_preview(session, true);
            self.drop_deferred();
            let _ = self.client.on_msg(msg);
            self.client.rewind_replica(self.version);
            // The replica is now behind the tail it just asked for: the host must treat this like
            // any other rollback, not like a settled step.
            return Effect::Rollback;
        }
        let effect = self.client.on_msg(msg);
        // A message that replaces or invalidates the replica ends a preview first; a snapshot
        // carrying a base then installs it, and the tail is replayed onto the fresh document.
        let invalidates = match &echo {
            ServerMsg::Welcome(_) | ServerMsg::Error(_) | ServerMsg::Snapshot(_) => true,
            ServerMsg::Rejected(rejected) => self.preview.as_ref().is_some_and(|preview| preview.op_id == rejected.op_id),
            _ => false,
        };
        if invalidates {
            self.end_preview(session, true);
        }
        if effect == Effect::Rollback && self.preview.is_some() {
            // The state machine has given up on the ordered stream (a gap, a fatal error): the
            // preview cannot be settled any more, so put it back before the resync lands.
            self.end_preview(session, true);
        }
        if let Some(ready) = self.client.take_ready_base() {
            self.install_base(session, ready);
        }
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
                if self.client.take_refused_snapshot() {
                    // The state machine refused this answer: it does not start at the version the
                    // host rewound its replica to, so installing its base or replaying its tail
                    // would leave the steps in between missing while the host claimed the head.
                    // It stays behind and the tail is asked for again from the version it holds.
                    return effect;
                }
                if snapshot.base.as_ref().is_some_and(BaseSnapshot::is_chained) {
                    // The base is on its way in chunks: the tail above it is replayed by
                    // `install_base` together with the base, so nothing may be applied to the old
                    // replica now.
                    return effect;
                }
                let mut usable = true;
                if let Some(base) = &snapshot.base {
                    match self.replace_from_base(session, base) {
                        Ok(()) => {
                            self.version = base.version;
                            self.last_apply_error = None;
                        }
                        Err(e) => {
                            self.last_apply_error = Some(e.to_string());
                            usable = false;
                        }
                    }
                }
                if usable {
                    for applied in &snapshot.tail {
                        self.apply_broadcast(session, applied);
                    }
                    if snapshot.tail.is_empty() {
                        // A base alone (or a tail with nothing new) is the whole state.
                        self.version = snapshot.version;
                    }
                }
            }
            ServerMsg::QueryResult { id, version, result, error } => {
                self.query_answers.push_back(QueryAnswer { id, version, result, error: error.map(|e| e.message) });
            }
            _ => {}
        }
        self.drain_deferred(session);
        effect
    }

    /// The base this session assembled from a chain of chunks, for the host to install. At most
    /// once per chain; `None` while a chain is still being assembled.
    pub fn take_ready_base(&mut self) -> Option<ReadyBase> {
        self.client.take_ready_base()
    }

    /// Whether a chained base is being assembled right now (the status bar can say "rebuilding").
    pub fn rebuilding(&self) -> bool {
        self.client.rebuilding()
    }

    /// Replace the replica with an assembled base and replay the tail that came with its
    /// snapshot, so the host ends at the head the snapshot named.
    fn install_base(&mut self, session: &mut Session, ready: ReadyBase) {
        let base = ready.snapshot.base.clone();
        let installed = match &base {
            Some(base) => self.replace_from_bytes(session, &ready.bytes, &base.keys).map(|()| base.version),
            None => Err(LocalError::BadParams("the assembled base has no snapshot".into())),
        };
        match installed {
            Ok(base_version) => {
                self.version = base_version;
                self.last_apply_error = None;
                for applied in &ready.snapshot.tail {
                    self.apply_broadcast(session, applied);
                }
                self.version = self.version.max(ready.snapshot.version);
            }
            Err(e) => self.last_apply_error = Some(e.to_string()),
        }
    }

    /// Rebuild the replica from the native base the server sent after compaction (ADR-0005
    /// §4.5). The base carries the session key map, so operations above it address the same
    /// objects as on every other replica.
    fn replace_from_base(&mut self, session: &mut Session, base: &BaseSnapshot) -> Result<(), LocalError> {
        /// A hostile or misconfigured server must not make the client allocate without a cap.
        const MAX_BASE: usize = 64 << 20;
        let Some(encoded) = base.bytes_base64.as_deref() else {
            return Err(LocalError::BadParams("the session was compacted past this replica; reopen the file from the cabinet".into()));
        };
        if encoded.len() > MAX_BASE {
            return Err(LocalError::BadParams("the session base is larger than this client accepts".into()));
        }
        let bytes =
            base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|_| LocalError::BadParams("the session base is not base64".into()))?;
        self.replace_from_bytes(session, &bytes, &base.keys)
    }

    /// Load native base bytes into the replica and take the session key map they carry.
    fn replace_from_bytes(&mut self, session: &mut Session, bytes: &[u8], keys: &SnapshotKeys) -> Result<(), LocalError> {
        let loaded = deckcraft_engine::cmd::file::loaded_file(&format!("{}.base", self.file_id), bytes)
            .map_err(|e| LocalError::BadParams(format!("the session base does not load: {e}")))?;
        session.replace_active_document(loaded.doc).map_err(|e| LocalError::BadParams(e.to_string()))?;
        self.adapter.restore_keys(keys);
        Ok(())
    }

    /// Apply one confirmed step. When it is the operation this replica shows, the local model is
    /// already its result: only the version moves (RFC §3). If the room ran a different form — a
    /// rebase this client did not expect — the preview is put back and the authoritative command
    /// runs, so the replica ends where a client without preview would have been.
    fn apply_broadcast(&mut self, session: &mut Session, applied: &Applied) {
        if self.preview.as_ref().is_some_and(|preview| preview.op_id == applied.op_id)
            && let Some(preview) = self.preview.take()
        {
            let settled = applied.version == self.version + 1
                && applied.command.as_ref().is_some_and(|command| command.payload == preview.payload && command.target == preview.target);
            if settled {
                self.version = applied.version;
                self.last_apply_error = None;
                return;
            }
            self.restore_before(session, preview.before);
        }
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
    normalize_with_painter(keys, st, None, cmd, p, op_id, doc)
}

/// [`normalize`] with the Animation Painter clipboard, which lives in the session and not in the
/// document: `animation.painterApply` normalises the picked steps into the slide's whole resulting
/// sequence, so no replica needs the clipboard itself.
pub fn normalize_with_painter(
    keys: &crate::KeyMap,
    st: &DocState,
    painter: Option<&(Vec<Animation>, bool)>,
    cmd: &str,
    p: &Value,
    op_id: &str,
    doc: &str,
) -> Result<Vec<OpEnvelope>, LocalError> {
    // A document font is a presentation value: it belongs to no slide, so it is normalised
    // before the slide-view guard and carries no target at all.
    if matches!(cmd, "font.create" | "font.set" | "font.remove") {
        let payload = font_payload(cmd, p)?;
        return Ok(vec![OpEnvelope {
            op_id: op_id.to_string(),
            doc: doc.to_string(),
            base_version: 0,
            kind: class_of(&payload),
            target: Target::default(),
            payload,
            history: None,
            preview: None,
        }]);
    }
    if st.selection.target != EngineTarget::Slides {
        return Err(LocalError::Unsupported(cmd.into()));
    }
    let slide = st.doc.slides.get(st.selection.slide).ok_or(LocalError::NoDocument)?;
    let slide_key = keys.slide_key(slide.id).cloned();
    // One operation is one identity: the room remembers accepted operations by `opId` and answers
    // a repeat with the stored result instead of applying it again (ADR 0003). A command that
    // normalises to N operations — a move or a resize of a multiple selection, a deletion of
    // several slides or animation steps — must therefore carry N different ids, or the room would
    // apply only the first and answer the rest with its version, while this replica showed them
    // all. The first operation keeps the id the caller handed in: the optimistic preview and the
    // sender's own settle are keyed on it (RFC §3). The numbering is per command and counts from
    // the first emitted operation, so normalising the same command twice gives the same ids and a
    // resend is still one operation.
    let seq = std::cell::Cell::new(0u32);
    let make = |target: Target, payload: Payload| {
        let index = seq.get();
        seq.set(index + 1);
        OpEnvelope {
            op_id: if index == 0 { op_id.to_string() } else { format!("{op_id}#{index}") },
            doc: doc.to_string(),
            base_version: 0,
            kind: class_of(&payload),
            target,
            payload,
            history: None,
            preview: None,
        }
    };
    // A whole-value object — the rest of the table surface, a SmartArt graphic and a chart — is one
    // final value per command, every address resolved from this replica's selection, so the
    // operation says exactly which object it changes and a replica never merges arguments against
    // its own (possibly imported) copy.
    if let Some(result) = whole_value(cmd, p, slide, st) {
        let (id, payload) = result?;
        return Ok(vec![make(Target { slide: slide_key, shape: Some(shape_key(keys, slide.id, id)?), ..Target::default() }, payload)]);
    }
    // The three conversions replace one shape with another, so they are their own structural
    // step: the sender runs the same engine plan the single-user command runs and sends the whole
    // result (a diagram, a text body or nothing), never arguments a replica would apply to its
    // own copy (RFC §15).
    if let Some(result) = smartart_conversion(keys, cmd, p, slide_key.clone(), st, op_id, doc) {
        return result.map(|op| vec![op]);
    }
    // One node's picture is its own final value: the media channel's digest, the top-level node
    // and the picture's place. It is not a structural step: the group keeps its place and id, and
    // the undo image is the whole group.
    if cmd == "smartArt.picture" {
        return Ok(vec![smartart_picture(keys, p, slide_key, st, op_id, doc)?]);
    }
    // A finished gesture (a shape drag) names the boxes it left: one final-value operation, so the
    // room orders the result every replica draws instead of one replica keeping the move to itself.
    if cmd == "shape.setBoundsMany" {
        return drag_bounds(keys, p, slide, slide_key, op_id, doc);
    }
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
                .and_then(|a| Some((u32::try_from(a.first()?.as_u64()?).ok()?, u32::try_from(a.get(1)?.as_u64()?).ok()?)))
                .or_else(|| st.selection.text.as_ref().filter(|t| t.shape == id).and_then(|t| t.cell).map(|(r, c)| (r as u32, c as u32)));
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
                if ts.notes {
                    let body = Some(&slide.notes);
                    let (from, to) = (position_offset(body, ts.ordered().0), position_offset(body, ts.ordered().1));
                    if to <= from {
                        return Err(LocalError::BadParams("nothing selected".into()));
                    }
                    return Ok(vec![make(
                        Target { slide: slide_key, ..Target::default() },
                        Payload::NotesDelete { from: from as u32, to: to as u32 },
                    )]);
                }
                let (body, cell) = selected_body(&slide.shapes, ts)?;
                let (from, to) = (position_offset(body, ts.ordered().0), position_offset(body, ts.ordered().1));
                if to <= from {
                    return Err(LocalError::BadParams("nothing selected".into()));
                }
                return Ok(vec![make(
                    Target { slide: slide_key, shape: Some(shape_key(keys, slide.id, ts.shape)?), ..Target::default() },
                    Payload::TextDelete { from: from as u32, to: to as u32, cell },
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
            let text = p.get("text").and_then(Value::as_str).ok_or_else(|| LocalError::BadParams("text.insert needs `text`".into()))?;
            if ts.notes {
                // Typing in the notes pane is one address, the current slide's notes body.
                let at = position_offset(Some(&slide.notes), ts.caret);
                return Ok(vec![make(
                    Target { slide: slide_key, ..Target::default() },
                    Payload::NotesInsert { at: at as u32, text: text.to_string() },
                )]);
            }
            let (body, cell) = selected_body(&slide.shapes, ts)?;
            let at = position_offset(body, ts.caret);
            Ok(vec![make(
                Target { slide: slide_key, shape: Some(shape_key(keys, slide.id, ts.shape)?), ..Target::default() },
                Payload::TextInsert { at: at as u32, text: text.to_string(), cell },
            )])
        }
        "text.delete" => {
            let ts = st.selection.text.as_ref().ok_or_else(|| LocalError::BadParams("text.delete needs a text selection".into()))?;
            let (from, to) = (ts.ordered().0, ts.ordered().1);
            if ts.notes {
                let body = Some(&slide.notes);
                let (from, to) = (position_offset(body, from), position_offset(body, to));
                if to <= from {
                    return Err(LocalError::BadParams("nothing selected".into()));
                }
                return Ok(vec![make(Target { slide: slide_key, ..Target::default() }, Payload::NotesDelete { from: from as u32, to: to as u32 })]);
            }
            let (body, cell) = selected_body(&slide.shapes, ts)?;
            let (from, to) = (position_offset(body, from), position_offset(body, to));
            if to <= from {
                return Err(LocalError::BadParams("nothing selected".into()));
            }
            Ok(vec![make(
                Target { slide: slide_key, shape: Some(shape_key(keys, slide.id, ts.shape)?), ..Target::default() },
                Payload::TextDelete { from: from as u32, to: to as u32, cell },
            )])
        }
        "slide.notes" => {
            // The notes pane edits a whole slide body: the operation carries the complete body.
            let (key, value) = notes_body(keys, st, p)?;
            Ok(vec![make(Target { slide: Some(key), ..Target::default() }, Payload::NotesSet { notes: value })])
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
        "insert.chart" => Ok(vec![chart_insert(p, slide_key, op_id, doc)?]),
        "insert.smartArt" => Ok(vec![smartart_insert(p, slide_key, op_id, doc)?]),
        "insert.picture" | "insert.audio" | "insert.video" | "insert.model3d" => {
            let sha = p.get("sha256").and_then(Value::as_str).ok_or_else(|| LocalError::BadParams(format!("{cmd} needs a media digest")))?;
            if !crate::media::valid_sha(sha) {
                return Err(LocalError::BadParams(format!("{cmd}: the media digest is not a sha256")));
            }
            let name = p.get("name").and_then(Value::as_str).unwrap_or("media").to_string();
            let rect = match p.get("rect") {
                None => None,
                Some(value) => {
                    let array =
                        value.as_array().filter(|a| a.len() == 4).ok_or_else(|| LocalError::BadParams(format!("{cmd} rect must be [x,y,w,h]")))?;
                    let mut r = [0.0; 4];
                    for (i, v) in array.iter().enumerate() {
                        r[i] = v.as_f64().ok_or_else(|| LocalError::BadParams(format!("{cmd} rect values must be numbers")))?;
                    }
                    Some(r)
                }
            };
            for key in p.as_object().map(|o| o.keys()).into_iter().flatten() {
                if !matches!(key.as_str(), "sha256" | "name" | "rect") {
                    return Err(LocalError::Unsupported(format!("{cmd} {key}")));
                }
            }
            let target = Target { slide: slide_key, ..Target::default() };
            let payload = match cmd {
                "insert.picture" => Payload::PictureInsert { sha256: sha.to_string(), name, rect },
                "insert.video" => Payload::MediaInsert { kind: MediaKind::Video, sha256: sha.to_string(), name, rect },
                "insert.model3d" => Payload::MediaInsert { kind: MediaKind::Model, sha256: sha.to_string(), name, rect },
                _ => Payload::MediaInsert { kind: MediaKind::Audio, sha256: sha.to_string(), name, rect },
            };
            Ok(vec![make(target, payload)])
        }
        "model3d.camera" => {
            // The whole final view, validated by the same function the single-user command runs:
            // a relative field (dx/dy/zoom/orbit), a degenerate camera or a value out of range is
            // refused here, before an operation is sent, and the address must be one 3-D model.
            let ids = shape_ids(p, st)?;
            let [id] = ids.as_slice() else {
                return Err(LocalError::Unsupported("a 3-D model camera on several shapes".into()));
            };
            let shape = find_shape(&slide.shapes, *id).ok_or_else(|| LocalError::BadParams("no such shape".into()))?;
            if !matches!(shape.kind, ShapeKind::Model3d { .. }) {
                return Err(LocalError::BadParams("the shape is not a 3-D model".into()));
            }
            let camera = deckcraft_engine::cmd::model3d::camera_param(p)
                .map_err(|e| LocalError::BadParams(e.to_string()))?
                .map(|c| deckcraft_collab_proto::ModelCamera { position: c.position, target: c.target, up: c.up, fov: c.fov });
            Ok(vec![make(
                Target { slide: slide_key, shape: Some(shape_key(keys, slide.id, *id)?), ..Target::default() },
                Payload::Model3dCamera { camera },
            )])
        }
        "model3d.scene" => {
            // The whole final scene, validated by the same function the single-user command runs:
            // a relative field (dimmer/rotate/zoom), a light that cannot be drawn or a value out
            // of range is refused here, before an operation is sent, and the address must be one
            // 3-D model.
            let ids = shape_ids(p, st)?;
            let [id] = ids.as_slice() else {
                return Err(LocalError::Unsupported("a 3-D model scene on several shapes".into()));
            };
            let shape = find_shape(&slide.shapes, *id).ok_or_else(|| LocalError::BadParams("no such shape".into()))?;
            if !matches!(shape.kind, ShapeKind::Model3d { .. }) {
                return Err(LocalError::BadParams("the shape is not a 3-D model".into()));
            }
            let scene = deckcraft_engine::cmd::model3d::scene_param(p).map_err(|e| LocalError::BadParams(e.to_string()))?.map(wire_scene);
            Ok(vec![make(
                Target { slide: slide_key, shape: Some(shape_key(keys, slide.id, *id)?), ..Target::default() },
                Payload::Model3dScene { scene },
            )])
        }
        "shape.fill" => {
            let fill = fill_spec(p)?;
            let ids = shape_ids(p, st)?;
            if ids.is_empty() {
                return Err(LocalError::BadParams("shape.fill needs a shape".into()));
            }
            let mut out = Vec::new();
            for id in ids {
                out.push(make(
                    Target { slide: slide_key.clone(), shape: Some(shape_key(keys, slide.id, id)?), ..Target::default() },
                    Payload::ShapeFill { fill: fill.clone() },
                ));
            }
            Ok(out)
        }
        "shape.line" => {
            let line = line_spec(p)?;
            let ids = shape_ids(p, st)?;
            if ids.is_empty() {
                return Err(LocalError::BadParams("shape.line needs a shape".into()));
            }
            let mut out = Vec::new();
            for id in ids {
                out.push(make(
                    Target { slide: slide_key.clone(), shape: Some(shape_key(keys, slide.id, id)?), ..Target::default() },
                    Payload::ShapeLine { line: line.clone() },
                ));
            }
            Ok(out)
        }
        "transition.set" => {
            // The whole resulting transition, computed with the same planning function the
            // single-user command runs: `transition.set` keeps the advance timings, the embedded
            // sound and the kept XML of the value it found, so sending its arguments would let
            // each replica merge them against a different state.
            let index = match p.get("index") {
                None | Some(Value::Null) => {
                    if st.selection.slides.len() > 1 {
                        return Err(LocalError::Unsupported("a transition on several selected slides".into()));
                    }
                    st.selection.slides.first().copied().and_then(|id| st.doc.slide_index(id)).unwrap_or(st.selection.slide)
                }
                Some(v) => v
                    .as_u64()
                    .and_then(|v| usize::try_from(v).ok())
                    .ok_or_else(|| LocalError::BadParams(format!("`index` is not a valid slide index: {v}")))?,
            };
            let target = st.doc.slides.get(index).ok_or_else(|| LocalError::BadParams("no such slide".into()))?;
            let plan =
                deckcraft_engine::cmd::transition::set_plan(target.transition.as_ref(), p).map_err(|e| LocalError::BadParams(e.to_string()))?;
            let transition = match plan {
                Some(value) => {
                    Some(crate::transition::spec(&value).ok_or_else(|| LocalError::BadParams("the transition value cannot be sent".into()))?)
                }
                None => None,
            };
            let key = keys.slide_key(target.id).cloned().ok_or(LocalError::NoDocument)?;
            Ok(vec![make(Target { slide: Some(key), ..Target::default() }, Payload::SlideTransition { transition })])
        }
        "animation.add" | "animation.remove" => {
            // One shape per operation: the wire value is one shape's whole step list, and a
            // selection of several shapes would become several versions and several undo steps.
            let (id, drop_index) = match p.get("index") {
                None | Some(Value::Null) => {
                    let ids = shape_ids(p, st)?;
                    let [id] = ids.as_slice() else {
                        return Err(LocalError::Unsupported(format!("{cmd} on several shapes")));
                    };
                    (*id, None)
                }
                Some(value) => {
                    let index = value
                        .as_u64()
                        .and_then(|v| usize::try_from(v).ok())
                        .ok_or_else(|| LocalError::BadParams(format!("`index` is not a valid animation index: {value}")))?;
                    let at = slide.animations.get(index).ok_or_else(|| LocalError::BadParams("no such animation".into()))?;
                    (at.shape, Some(index))
                }
            };
            find_shape(&slide.shapes, id).ok_or_else(|| LocalError::BadParams("no such shape".into()))?;
            let key = shape_key(keys, slide.id, id)?;
            let steps: Vec<Animation> = if cmd == "animation.add" {
                let size = &st.doc.slide_size;
                let mut steps: Vec<Animation> = slide.animations.iter().filter(|a| a.shape == id).cloned().collect();
                let mut step =
                    deckcraft_engine::cmd::animation::add_plan(p, (size.width, size.height)).map_err(|e| LocalError::BadParams(e.to_string()))?;
                step.shape = id;
                steps.push(step);
                steps
            } else {
                slide.animations.iter().enumerate().filter(|(k, a)| a.shape == id && Some(*k) != drop_index).map(|(_, a)| a.clone()).collect()
            };
            let current = slide.animations.iter().filter(|a| a.shape == id).count();
            if steps.len() == current && drop_index.is_none() {
                // Nothing to remove: a shape without steps (or a step already gone) sends nothing.
                return Ok(Vec::new());
            }
            let animations = steps
                .iter()
                .map(crate::animation::spec)
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| LocalError::BadParams("the animation steps cannot be sent".into()))?;
            Ok(vec![make(Target { slide: slide_key, shape: Some(key), ..Target::default() }, Payload::ShapeAnimations { animations })])
        }
        "animation.set" => {
            // The whole resulting sequence of the slide: `animation.set` replaces each addressed
            // shape's steps in place, and a slide-wide list keeps the steps of every other shape
            // exactly where they were, interleaving included.
            let ids = shape_ids(p, st)?;
            for id in &ids {
                find_shape(&slide.shapes, *id).ok_or_else(|| LocalError::BadParams("no such shape".into()))?;
            }
            let size = (st.doc.slide_size.width, st.doc.slide_size.height);
            let animations =
                deckcraft_engine::cmd::animation::set_plan(p, &slide.animations, &ids, size).map_err(|e| LocalError::BadParams(e.to_string()))?;
            if animations == slide.animations {
                return Ok(Vec::new());
            }
            animation_list_ops(keys, slide, &animations, &make)
        }
        "animation.timing" => {
            let index = match p.get("index") {
                None | Some(Value::Null) => None,
                Some(value) => Some(
                    value
                        .as_u64()
                        .and_then(|v| usize::try_from(v).ok())
                        .ok_or_else(|| LocalError::BadParams(format!("`index` is not a valid animation index: {value}")))?,
                ),
            };
            let ids: Vec<ShapeId> = if index.is_none() { shape_ids(p, st)? } else { Vec::new() };
            let (animations, _) =
                deckcraft_engine::cmd::animation::timing_plan(p, &slide.animations, index, &ids).map_err(|e| LocalError::BadParams(e.to_string()))?;
            if animations == slide.animations {
                return Ok(Vec::new());
            }
            animation_list_ops(keys, slide, &animations, &make)
        }
        "animation.move" => {
            let index = match p.get("index") {
                None => return Err(LocalError::BadParams("animation.move needs `index`".into())),
                Some(value) => value
                    .as_u64()
                    .and_then(|v| usize::try_from(v).ok())
                    .ok_or_else(|| LocalError::BadParams(format!("`index` is not a valid animation index: {value}")))?,
            };
            let to = p
                .get("to")
                .and_then(Value::as_u64)
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(|| LocalError::BadParams("animation.move needs `to`".into()))?;
            let animations =
                deckcraft_engine::cmd::animation::move_plan(&slide.animations, index, to).map_err(|e| LocalError::BadParams(e.to_string()))?;
            if animations == slide.animations {
                return Ok(Vec::new());
            }
            animation_list_ops(keys, slide, &animations, &make)
        }
        "animation.options" | "animation.clear" | "animation.painterApply" => animation_ops(keys, st, slide, painter, cmd, p, &make),
        cmd if cmd.starts_with("format.") => {
            let format = FormatCmd::from_id(cmd).ok_or_else(|| LocalError::Unsupported(cmd.into()))?;
            format_ops(keys, st, slide, format, p, &make)
        }
        "slide.duplicate" => {
            let index = p.get("index").and_then(Value::as_u64).map(|v| v as usize).unwrap_or(st.selection.slide);
            let slide_id = st.doc.slides.get(index).map(|s| s.id).ok_or_else(|| LocalError::BadParams("no such slide".into()))?;
            let key = keys.slide_key(slide_id).cloned().ok_or(LocalError::NoDocument)?;
            Ok(vec![make(Target { slide: Some(key), ..Target::default() }, Payload::SlideDuplicate {})])
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
        "arrange.align" | "arrange.distribute" => {
            let ids = shape_ids(p, st)?;
            if ids.is_empty() {
                return Err(LocalError::BadParams(format!("{cmd} needs a shape")));
            }
            // Final boxes come from the engine's own plan, so a shared session reaches exactly the
            // values the single-user command would set. One operation for the whole selection
            // means one version and one undo step (RFC §3.2.1, §13 item 4).
            let plan = if cmd == "arrange.align" {
                deckcraft_engine::cmd::arrange::align_plan(
                    st,
                    &ids,
                    p.get("edge").and_then(Value::as_str).unwrap_or("left"),
                    p.get("to").and_then(Value::as_str),
                )
            } else {
                deckcraft_engine::cmd::arrange::distribute_plan(st, &ids, p.get("dir").and_then(Value::as_str))
            };
            let mut items = Vec::new();
            for (id, xf) in plan {
                let shape = find_shape(&slide.shapes, id).ok_or_else(|| LocalError::BadParams("no such shape".into()))?;
                if shape.locked {
                    continue;
                }
                let now = current_xfrm(&st.doc, st, shape);
                if xf.x == now.x && xf.y == now.y && xf.w == now.w && xf.h == now.h {
                    continue;
                }
                items.push(ShapeBounds { shape: shape_key(keys, slide.id, id)?, x: xf.x, y: xf.y, w: xf.w, h: xf.h });
            }
            if items.is_empty() {
                return Ok(Vec::new());
            }
            let shapes = items.iter().map(|b| b.shape.clone()).collect();
            Ok(vec![make(Target { slide: slide_key, shapes, ..Target::default() }, Payload::ShapesSetBounds { items })])
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
        "table.insertRowAbove"
        | "table.insertRowBelow"
        | "table.deleteRow"
        | "table.insertColumnLeft"
        | "table.insertColumnRight"
        | "table.deleteColumn" => {
            let cell = st.selection.text.as_ref().and_then(|t| t.cell.map(|c| (t.shape, c))).or_else(|| st.selection.cells.map(|c| (c.0, c.1)));
            let id = p
                .get("id")
                .and_then(Value::as_u64)
                .map(|v| ShapeId(v as u32))
                .or(cell.map(|c| c.0))
                .or_else(|| st.selection.shapes.first().copied())
                .ok_or_else(|| LocalError::BadParams(format!("{cmd} needs a table")))?;
            let shape = find_shape(&slide.shapes, id).ok_or_else(|| LocalError::BadParams("no such shape".into()))?;
            let ShapeKind::Table(t) = &shape.kind else {
                return Err(LocalError::BadParams(format!("{cmd} needs a table")));
            };
            let (n_rows, n_cols) = (t.n_rows(), t.n_cols());
            let at_cell = cell.map(|c| c.1).unwrap_or((0, 0));
            let row = p.get("row").and_then(Value::as_u64).map(|v| v as usize).unwrap_or(at_cell.0);
            let col = p.get("col").and_then(Value::as_u64).map(|v| v as usize).unwrap_or(at_cell.1);
            let (axis, at, insert) = match cmd {
                "table.insertRowAbove" => (TableAxis::Row, row, true),
                "table.insertRowBelow" => (TableAxis::Row, row.saturating_add(1), true),
                "table.deleteRow" => {
                    if row >= n_rows {
                        return Err(LocalError::BadParams(format!("the table has no row {row}")));
                    }
                    (TableAxis::Row, row, false)
                }
                "table.insertColumnLeft" => (TableAxis::Column, col, true),
                "table.insertColumnRight" => (TableAxis::Column, col.saturating_add(1), true),
                _ => {
                    if col >= n_cols {
                        return Err(LocalError::BadParams(format!("the table has no column {col}")));
                    }
                    (TableAxis::Column, col, false)
                }
            };
            let limit = match axis {
                TableAxis::Row => n_rows,
                TableAxis::Column => n_cols,
            };
            if insert && at > limit {
                return Err(LocalError::BadParams(format!("{cmd}: no position {at} on that axis")));
            }
            Ok(vec![make(
                Target { slide: slide_key, shape: Some(shape_key(keys, slide.id, id)?), ..Target::default() },
                Payload::TableEdit { axis, at: u32::try_from(at).map_err(|_| LocalError::BadParams("the table is too large".into()))?, insert },
            )])
        }
        other => Err(LocalError::Unsupported(other.into())),
    }
}

/// The SmartArt commands that leave the graphic as one value: the outline, its layout, style and
/// colours. `None` when the command is not one of them. The sender resolves the group from an
/// explicit id or its own selection and sends the resulting whole `Diagram`, so a replica's
/// selection and its stale copy decide nothing (RFC §15).
fn smartart_value(cmd: &str, p: &Value, slide: &Slide, st: &DocState) -> Option<Result<(ShapeId, Payload), LocalError>> {
    const COMMANDS: [&str; 8] = [
        "smartArt.setNodes",
        "smartArt.addNode",
        "smartArt.removeNode",
        "smartArt.promote",
        "smartArt.demote",
        "smartArt.layout",
        "smartArt.style",
        "smartArt.colors",
    ];
    COMMANDS.contains(&cmd).then(|| smartart_payload(cmd, p, slide, st))
}

fn smartart_payload(cmd: &str, p: &Value, slide: &Slide, st: &DocState) -> Result<(ShapeId, Payload), LocalError> {
    let ids = shape_ids(p, st)?;
    let group = ids
        .iter()
        .find_map(|id| deckcraft_engine::cmd::smartart::group_of(&slide.shapes, *id))
        .ok_or_else(|| LocalError::BadParams(format!("{cmd} needs a SmartArt graphic")))?;
    let mut diagram =
        deckcraft_engine::cmd::smartart::synced(group).ok_or_else(|| LocalError::BadParams(format!("{cmd} needs a SmartArt graphic")))?;
    // The same plan the single-user command runs, so the value sent is the value that command
    // would leave; a refusal (an unknown layout, a node that does not exist) happens here, before
    // anything is queued.
    deckcraft_engine::cmd::smartart::plan(cmd, p, &mut diagram).map_err(|e| LocalError::BadParams(e.to_string()))?;
    let value = serde_json::to_value(&diagram).map_err(|e| LocalError::BadParams(format!("{cmd}: {e}")))?;
    Ok((group.id, Payload::SmartArtSet { diagram: value }))
}

/// The chart commands that leave the chart as one value: its type, data and options. `None` when
/// the command is not one of them. The sender resolves the chart from an explicit id or its own
/// selection, runs the engine's plan on the chart it sees and sends the whole resulting `Chart`,
/// so a replica's selection and its stale copy decide nothing (RFC §3.2.1).
fn chart_value(cmd: &str, p: &Value, slide: &Slide, st: &DocState) -> Option<Result<(ShapeId, Payload), LocalError>> {
    const COMMANDS: [&str; 3] = ["chart.type", "chart.data", "chart.options"];
    COMMANDS.contains(&cmd).then(|| chart_payload(cmd, p, slide, st))
}

fn chart_payload(cmd: &str, p: &Value, slide: &Slide, st: &DocState) -> Result<(ShapeId, Payload), LocalError> {
    let ids = shape_ids(p, st)?;
    let shape = ids
        .iter()
        .filter_map(|id| find_shape(&slide.shapes, *id))
        .find(|sh| matches!(sh.kind, ShapeKind::Chart(_)))
        .ok_or_else(|| LocalError::BadParams(format!("{cmd} needs a chart")))?;
    let ShapeKind::Chart(existing) = &shape.kind else {
        return Err(LocalError::BadParams(format!("{cmd} needs a chart")));
    };
    let mut chart = (**existing).clone();
    // The same plan the single-user command runs, so the value sent is the value that command
    // would leave; a refusal (a bad bin size, a bad text colour) happens here, before anything is
    // queued.
    deckcraft_engine::cmd::insert::chart_plan(cmd, p, &mut chart).map_err(|e| LocalError::BadParams(e.to_string()))?;
    let value = serde_json::to_value(&chart).map_err(|e| LocalError::BadParams(format!("{cmd}: {e}")))?;
    Ok((shape.id, Payload::ChartSet { chart: value }))
}

/// The whole chart a collaborative `insert.chart` creates: the sender runs the single-user plan
/// on the chart its command would build and sends the resulting value, so every replica creates
/// exactly that chart instead of running its own sample. A parameter the plan does not carry is
/// refused rather than silently dropped (RFC §3.2.1).
fn chart_insert(p: &Value, slide_key: Option<SlideKey>, op_id: &str, doc: &str) -> Result<OpEnvelope, LocalError> {
    let mut chart = deckcraft_model::chart::Chart::default();
    deckcraft_engine::cmd::insert::chart_plan("insert.chart", p, &mut chart).map_err(|e| LocalError::BadParams(e.to_string()))?;
    for key in p.as_object().map(|o| o.keys()).into_iter().flatten() {
        if !matches!(key.as_str(), "type" | "categories" | "series" | "title" | "rect") {
            return Err(LocalError::Unsupported(format!("insert.chart {key}")));
        }
    }
    let rect = match p.get("rect") {
        None => None,
        Some(value) => {
            let array =
                value.as_array().filter(|a| a.len() == 4).ok_or_else(|| LocalError::BadParams("insert.chart rect must be [x,y,w,h]".into()))?;
            let mut r = [0.0; 4];
            for (i, v) in array.iter().enumerate() {
                r[i] = v.as_f64().ok_or_else(|| LocalError::BadParams("insert.chart rect values must be numbers".into()))?;
            }
            Some(r)
        }
    };
    let chart = serde_json::to_value(&chart).map_err(|e| LocalError::BadParams(format!("insert.chart: {e}")))?;
    Ok(OpEnvelope {
        op_id: op_id.to_string(),
        doc: doc.to_string(),
        base_version: 0,
        kind: OpKind::Structural,
        target: Target { slide: slide_key, ..Target::default() },
        payload: Payload::ChartInsert { chart, rect },
        history: None,
        preview: None,
    })
}

/// The whole diagram a collaborative `insert.smartArt` creates: the sender runs the single-user
/// plan on the graphic its command would build and sends the resulting value, so every replica
/// creates exactly that drawing instead of running its own sample. The plan is the same function
/// the single-user command calls, so a layout, an outline and a default all travel decided. A
/// parameter the plan does not carry is refused rather than silently dropped (RFC §3.2.1).
fn smartart_insert(p: &Value, slide_key: Option<SlideKey>, op_id: &str, doc: &str) -> Result<OpEnvelope, LocalError> {
    for key in p.as_object().map(|o| o.keys()).into_iter().flatten() {
        if !matches!(key.as_str(), "kind" | "items" | "nodes" | "rect") {
            return Err(LocalError::Unsupported(format!("insert.smartArt {key}")));
        }
    }
    let diagram = deckcraft_engine::cmd::smartart::insert_plan(p).map_err(|e| LocalError::BadParams(e.to_string()))?;
    let rect = match p.get("rect") {
        None => None,
        Some(value) => {
            let array =
                value.as_array().filter(|a| a.len() == 4).ok_or_else(|| LocalError::BadParams("insert.smartArt rect must be [x,y,w,h]".into()))?;
            let mut r = [0.0; 4];
            for (i, v) in array.iter().enumerate() {
                r[i] = v.as_f64().ok_or_else(|| LocalError::BadParams("insert.smartArt rect values must be numbers".into()))?;
            }
            Some(r)
        }
    };
    let diagram = serde_json::to_value(&diagram).map_err(|e| LocalError::BadParams(format!("insert.smartArt: {e}")))?;
    Ok(OpEnvelope {
        op_id: op_id.to_string(),
        doc: doc.to_string(),
        base_version: 0,
        kind: OpKind::Structural,
        target: Target { slide: slide_key, ..Target::default() },
        payload: Payload::SmartArtInsert { diagram, rect },
        history: None,
        preview: None,
    })
}

/// One `smartArt.picture` step: the addressed top-level SmartArt group, one top-level node of its
/// outline, and the picture's final value — the media channel's digest and the place in the
/// node's placeholder (`None` = reset). The bytes are already on the media channel (`submit` put
/// them there), so the operation stays small and every replica installs the same object. The
/// sender resolves the node from its own selection, so a replica's selection decides nothing
/// (RFC §3.2.1, §15).
fn smartart_picture(
    keys: &crate::KeyMap,
    p: &Value,
    slide_key: Option<SlideKey>,
    st: &DocState,
    op_id: &str,
    doc: &str,
) -> Result<OpEnvelope, LocalError> {
    const CMD: &str = "smartArt.picture";
    let slide = st.doc.slides.get(st.selection.slide).ok_or(LocalError::NoDocument)?;
    let ids = shape_ids(p, st)?;
    let group = ids
        .iter()
        .find_map(|id| deckcraft_engine::cmd::smartart::group_of(&slide.shapes, *id))
        .ok_or_else(|| LocalError::BadParams(format!("{CMD} needs a SmartArt graphic")))?;
    // The confirmed command only works on a top-level graphic: a nested one has no place on the
    // slide for a picture step to address.
    if !slide.shapes.iter().any(|s| s.id == group.id) {
        return Err(LocalError::Unsupported(format!("{CMD} on a nested SmartArt graphic")));
    }
    let index = p
        .get("index")
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| LocalError::BadParams(format!("{CMD} needs `index`")))?;
    let diagram = deckcraft_engine::cmd::smartart::synced(group).ok_or_else(|| LocalError::BadParams(format!("{CMD} needs a SmartArt graphic")))?;
    let node = deckcraft_engine::cmd::smartart::top_node(&diagram, index).ok_or_else(|| LocalError::BadParams("no such node".into()))?;
    for key in p.as_object().map(|o| o.keys()).into_iter().flatten() {
        if !matches!(key.as_str(), "id" | "ids" | "index" | "sha256" | "name" | "place" | "reset") {
            return Err(LocalError::Unsupported(format!("{CMD} {key}")));
        }
    }
    let media = if p.get("reset").and_then(Value::as_bool).unwrap_or(false) {
        None
    } else {
        let sha = p.get("sha256").and_then(Value::as_str).ok_or_else(|| LocalError::BadParams(format!("{CMD} needs a media digest")))?;
        if !crate::media::valid_sha(sha) {
            return Err(LocalError::BadParams(format!("{CMD}: the media digest is not a sha256")));
        }
        let name = p.get("name").and_then(Value::as_str).unwrap_or("picture").to_string();
        // The same place check the engine runs, so a hostile place is refused before anything is
        // queued and the value on the wire is the value that command would accept.
        let place = deckcraft_engine::cmd::smartart::place_param(p, CMD).map_err(|e| LocalError::BadParams(e.to_string()))?.unwrap_or_default();
        Some(SmartArtPictureMedia { sha256: sha.to_string(), name, place: PicturePlace { crop: place.crop, alpha: place.alpha } })
    };
    let node = u32::try_from(node).map_err(|_| LocalError::BadParams("the outline is too large".into()))?;
    Ok(OpEnvelope {
        op_id: op_id.to_string(),
        doc: doc.to_string(),
        base_version: 0,
        kind: OpKind::Command,
        target: Target { slide: slide_key, shape: Some(shape_key(keys, slide.id, group.id)?), ..Target::default() },
        payload: Payload::SmartArtPicture { node, media },
        history: None,
        preview: None,
    })
}

/// The three SmartArt conversions: `fromText`, `toText` and `toShapes`. Each replaces one shape
/// with another in a single structural step. The sender runs the same engine plan the single-user
/// command runs — against the document and selection it sees — and sends the whole result (the
/// diagram or text body, and the final box) or, for `toShapes`, names the graphic to detach. A
/// refusal (no text, no graphic, a value past the engine's bounds) happens here, before anything is
/// queued, and the receiver checks the same bounds before the first write (RFC §15).
fn smartart_conversion(
    keys: &crate::KeyMap,
    cmd: &str,
    p: &Value,
    slide_key: Option<SlideKey>,
    st: &DocState,
    op_id: &str,
    doc: &str,
) -> Option<Result<OpEnvelope, LocalError>> {
    let slide = st.doc.slides.get(st.selection.slide)?;
    let make = |id: ShapeId, payload: Payload| -> Result<OpEnvelope, LocalError> {
        Ok(OpEnvelope {
            op_id: op_id.to_string(),
            doc: doc.to_string(),
            base_version: 0,
            kind: OpKind::Structural,
            target: Target { slide: slide_key.clone(), shape: Some(shape_key(keys, slide.id, id)?), ..Target::default() },
            payload,
            history: None,
            preview: None,
        })
    };
    match cmd {
        "smartArt.fromText" => Some((|| {
            let plan =
                deckcraft_engine::cmd::smartart::from_text_plan(&st.doc, &st.selection, p).map_err(|e| LocalError::BadParams(e.to_string()))?;
            let diagram = serde_json::to_value(&plan.diagram).map_err(|e| LocalError::BadParams(format!("smartArt.fromText: {e}")))?;
            let r = plan.rect;
            make(plan.id, Payload::SmartArtFromText { diagram, rect: [r.x, r.y, r.w, r.h] })
        })()),
        "smartArt.toText" => Some((|| {
            let plan = deckcraft_engine::cmd::smartart::to_text_plan(&st.doc, &st.selection, p).map_err(|e| LocalError::BadParams(e.to_string()))?;
            let body = serde_json::to_value(&plan.body).map_err(|e| LocalError::BadParams(format!("smartArt.toText: {e}")))?;
            let r = plan.rect;
            make(plan.id, Payload::SmartArtToText { body, rect: [r.x, r.y, r.w, r.h] })
        })()),
        "smartArt.toShapes" => Some((|| {
            let id = deckcraft_engine::cmd::smartart::to_shapes_plan(&st.doc, &st.selection, p).map_err(|e| LocalError::BadParams(e.to_string()))?;
            make(id, Payload::SmartArtToShapes {})
        })()),
        _ => None,
    }
}

/// The commands that leave one object as a whole value: the rest of the table surface, a SmartArt
/// graphic and a chart. `None` when the command is not one of them. The sender resolves the object
/// from an explicit id or its own selection and sends the whole resulting value, so a replica's
/// selection and its stale copy decide nothing (RFC §3.2.1, §6).
fn whole_value(cmd: &str, p: &Value, slide: &Slide, st: &DocState) -> Option<Result<(ShapeId, Payload), LocalError>> {
    table_value(cmd, p, slide, st).or_else(|| smartart_value(cmd, p, slide, st)).or_else(|| chart_value(cmd, p, slide, st))
}

/// The rest of the table surface, one final value per command: a style, six style options, one
/// cell fill over an explicit cell list, a merge or a split, and one column width or row height.
/// `None` when the command is not one of them. Every address is resolved from the sender's own
/// selection here, so the operation says which table and which cells it changes and a replica's
/// selection decides nothing (RFC §15).
fn table_value(cmd: &str, p: &Value, slide: &Slide, st: &DocState) -> Option<Result<(ShapeId, Payload), LocalError>> {
    Some(match cmd {
        "table.style" => table_style(p, slide, st),
        "table.options" => table_options(p, slide, st),
        "table.cellFill" => table_cell_fill(p, slide, st),
        "table.merge" => table_merge(p, slide, st),
        "table.split" => table_split(p, slide, st),
        "table.columnWidth" => table_column_width(p, slide, st),
        "table.rowHeight" => table_row_height(p, slide, st),
        _ => return None,
    })
}

fn table_style(p: &Value, slide: &Slide, st: &DocState) -> Result<(ShapeId, Payload), LocalError> {
    let (id, _) = table_target(slide, st, p, "table.style")?;
    let style = p.get("style").and_then(Value::as_str).map(str::trim).unwrap_or_default();
    if style.is_empty() || style.chars().count() > 255 || style.chars().any(char::is_control) {
        return Err(LocalError::BadParams("table.style needs a style name".into()));
    }
    Ok((id, Payload::TableStyle { style: style.to_string() }))
}

fn table_options(p: &Value, slide: &Slide, st: &DocState) -> Result<(ShapeId, Payload), LocalError> {
    let (id, table) = table_target(slide, st, p, "table.options")?;
    // The six toggles are independent, so the operation carries all of them as final values: the
    // sender applies the options it names to the table it sees and sends the resulting set. An
    // unknown flag or a non-boolean value is refused here, before anything is queued, and a call
    // that names no option sends nothing rather than an empty no-op operation.
    let Some(map) = p.as_object() else {
        return Err(LocalError::BadParams("table.options: params are an object".into()));
    };
    let mut flags = [None; 6];
    for (key, value) in map {
        if key == "id" {
            continue;
        }
        let slot = match key.as_str() {
            "headerRow" => 0,
            "totalRow" => 1,
            "bandedRows" => 2,
            "firstColumn" => 3,
            "lastColumn" => 4,
            "bandedColumns" => 5,
            other => return Err(LocalError::BadParams(format!("table.options: unknown option `{other}`"))),
        };
        let Some(v) = value.as_bool() else {
            return Err(LocalError::BadParams(format!("table.options: `{key}` is a boolean")));
        };
        flags[slot] = Some(v);
    }
    if flags.iter().all(Option::is_none) {
        return Err(LocalError::BadParams("table.options needs an option to set".into()));
    }
    let mut values = [table.first_row, table.last_row, table.band_row, table.first_col, table.last_col, table.band_col];
    for (value, flag) in values.iter_mut().zip(flags) {
        if let Some(v) = flag {
            *value = v;
        }
    }
    Ok((
        id,
        Payload::TableOptions {
            header_row: values[0],
            total_row: values[1],
            banded_rows: values[2],
            first_column: values[3],
            last_column: values[4],
            banded_columns: values[5],
        },
    ))
}

fn table_cell_fill(p: &Value, slide: &Slide, st: &DocState) -> Result<(ShapeId, Payload), LocalError> {
    let (id, table) = table_target(slide, st, p, "table.cellFill")?;
    let fill = if p.get("none").and_then(Value::as_bool).unwrap_or(false) {
        CellFillSpec::None
    } else if let Some(color) = p.get("color").and_then(Value::as_str) {
        let color = color.trim();
        if color.is_empty() || color.chars().count() > 64 || color.chars().any(char::is_control) {
            return Err(LocalError::BadParams("table.cellFill: `color` is not a colour".into()));
        }
        CellFillSpec::Solid { color: color.to_string() }
    } else {
        CellFillSpec::Inherit
    };
    let cells = fill_cells(st, id, table, p)?;
    Ok((id, Payload::TableCellFill { fill, cells }))
}

fn table_merge(p: &Value, slide: &Slide, st: &DocState) -> Result<(ShapeId, Payload), LocalError> {
    let (id, table) = table_target(slide, st, p, "table.merge")?;
    let selected = st.selection.cells.filter(|c| c.0 == id).map(|c| selection_cell(c.1));
    let from = cell_param(p.get("from"))?.or(selected).ok_or_else(|| LocalError::BadParams("table.merge needs `from`".into()))?;
    let to = cell_param(p.get("to"))?
        .or_else(|| st.selection.cells.filter(|c| c.0 == id).map(|c| selection_cell(c.2)))
        .ok_or_else(|| LocalError::BadParams("table.merge needs `to`".into()))?;
    for (r, c) in [from, to] {
        if r as usize >= table.n_rows() || c as usize >= table.n_cols() {
            return Err(LocalError::BadParams(format!("the table has no cell ({r}, {c})")));
        }
    }
    if from == to {
        return Err(LocalError::BadParams("a merge needs at least two cells".into()));
    }
    Ok((id, Payload::TableMerge { from, to }))
}

fn table_split(p: &Value, slide: &Slide, st: &DocState) -> Result<(ShapeId, Payload), LocalError> {
    let (id, table) = table_target(slide, st, p, "table.split")?;
    let caret = st.selection.text.as_ref().filter(|t| t.shape == id).and_then(|t| t.cell).map(selection_cell);
    let selected = st.selection.cells.filter(|c| c.0 == id).map(|c| selection_cell(c.1));
    let cell = cell_param(p.get("cell"))?.or(caret).or(selected).unwrap_or((0, 0));
    let Some(target) = table.cell(cell.0 as usize, cell.1 as usize) else {
        return Err(LocalError::BadParams(format!("the table has no cell ({}, {})", cell.0, cell.1)));
    };
    if target.grid_span.max(1) == 1 && target.row_span.max(1) == 1 {
        return Err(LocalError::BadParams("that cell is not merged".into()));
    }
    Ok((id, Payload::TableSplit { cell }))
}

fn table_column_width(p: &Value, slide: &Slide, st: &DocState) -> Result<(ShapeId, Payload), LocalError> {
    let (id, table) = table_target(slide, st, p, "table.columnWidth")?;
    let col = p
        .get("col")
        .and_then(Value::as_u64)
        .map(|v| v as usize)
        .or_else(|| st.selection.text.as_ref().filter(|t| t.shape == id).and_then(|t| t.cell).map(|c| c.1))
        .or_else(|| st.selection.cells.filter(|c| c.0 == id).map(|c| c.1.1))
        .unwrap_or(0);
    if col >= table.n_cols() {
        return Err(LocalError::BadParams(format!("the table has no column {col}")));
    }
    let width = p
        .get("width")
        .and_then(Value::as_f64)
        .filter(|w| w.is_finite())
        .ok_or_else(|| LocalError::BadParams("table.columnWidth needs a `width`".into()))?;
    let keep_total = p.get("keepTotal").and_then(Value::as_bool).unwrap_or(false);
    Ok((
        id,
        Payload::TableColumnWidth {
            col: u32::try_from(col).map_err(|_| LocalError::BadParams("the table is too large".into()))?,
            // The single-user command clamps the width; send the clamped value so the operation
            // carries the final width and not a request to clamp it.
            width: width.clamp(4.0, 4000.0),
            keep_total,
        },
    ))
}

fn table_row_height(p: &Value, slide: &Slide, st: &DocState) -> Result<(ShapeId, Payload), LocalError> {
    let (id, table) = table_target(slide, st, p, "table.rowHeight")?;
    let row = p
        .get("row")
        .and_then(Value::as_u64)
        .map(|v| v as usize)
        .or_else(|| st.selection.text.as_ref().filter(|t| t.shape == id).and_then(|t| t.cell).map(|c| c.0))
        .or_else(|| st.selection.cells.filter(|c| c.0 == id).map(|c| c.1.0))
        .unwrap_or(0);
    if row >= table.n_rows() {
        return Err(LocalError::BadParams(format!("the table has no row {row}")));
    }
    let height = p
        .get("height")
        .and_then(Value::as_f64)
        .filter(|h| h.is_finite())
        .ok_or_else(|| LocalError::BadParams("table.rowHeight needs a `height`".into()))?;
    Ok((
        id,
        Payload::TableRowHeight {
            row: u32::try_from(row).map_err(|_| LocalError::BadParams("the table is too large".into()))?,
            height: height.clamp(4.0, 4000.0),
        },
    ))
}

/// The table a table command acts on: the explicit `id`, the cell under the caret, the selected
/// cell range or the one selected shape — in that order, as the single-user commands resolve it.
fn table_target<'a>(slide: &'a Slide, st: &DocState, p: &Value, cmd: &str) -> Result<(ShapeId, &'a Table), LocalError> {
    let cell = st.selection.text.as_ref().and_then(|t| t.cell.map(|c| (t.shape, c))).or_else(|| st.selection.cells.map(|c| (c.0, c.1)));
    let id = p
        .get("id")
        .and_then(Value::as_u64)
        .map(|v| ShapeId(v as u32))
        .or(cell.map(|c| c.0))
        .or_else(|| st.selection.shapes.first().copied())
        .ok_or_else(|| LocalError::BadParams(format!("{cmd} needs a table")))?;
    let shape = find_shape(&slide.shapes, id).ok_or_else(|| LocalError::BadParams(format!("{cmd}: no such shape")))?;
    match &shape.kind {
        ShapeKind::Table(table) => Ok((id, table)),
        _ => Err(LocalError::BadParams(format!("{cmd} needs a table"))),
    }
}

/// One `[row, col]` cell parameter: `Ok(None)` when absent, an error when present but malformed,
/// so a bad address never falls back to the selection.
fn cell_param(v: Option<&Value>) -> Result<Option<(u32, u32)>, LocalError> {
    let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(None) };
    let idx = |x: &Value| x.as_u64().and_then(|x| u32::try_from(x).ok());
    match v.as_array().map(Vec::as_slice) {
        Some([r, c]) if let (Some(r), Some(c)) = (idx(r), idx(c)) => Ok(Some((r, c))),
        _ => Err(LocalError::BadParams(format!("`{v}` is not a [row, col] cell"))),
    }
}

/// A model cell index on the wire. Tables are bounded to 1000 lines, so a `usize` cell always
/// fits the wire's `u32`.
fn selection_cell(cell: (usize, usize)) -> (u32, u32) {
    (u32::try_from(cell.0).unwrap_or(u32::MAX), u32::try_from(cell.1).unwrap_or(u32::MAX))
}

/// The cells a `table.cellFill` acts on, always explicit: the `cells` list, else every cell of the
/// selected range (grown over merged cells, the engine's own rule), else the cell under the caret,
/// else every cell of the table. Bounded, so one operation and its undo always fit the wire.
fn fill_cells(st: &DocState, id: ShapeId, table: &Table, p: &Value) -> Result<Vec<(u32, u32)>, LocalError> {
    const MAX_FILL_CELLS: usize = 4096;
    let cells: Vec<(u32, u32)> = if p.get("cells").is_some_and(|v| !v.is_null()) {
        let list = p.get("cells").and_then(Value::as_array).ok_or_else(|| LocalError::BadParams("`cells` is a list of [row, col] cells".into()))?;
        let mut out = Vec::with_capacity(list.len().min(MAX_FILL_CELLS + 1));
        for cell in list {
            out.push(cell_param(Some(cell))?.ok_or_else(|| LocalError::BadParams("a `cells` entry is not a [row, col] cell".into()))?);
        }
        out
    } else if let Some(((r0, c0), (r1, c1))) = deckcraft_engine::cmd::table::selected_range(st.selection.cells, id, table) {
        (r0..=r1).flat_map(|r| (c0..=c1).map(move |c| (r as u32, c as u32))).collect()
    } else if let Some(cell) = st.selection.text.as_ref().filter(|t| t.shape == id).and_then(|t| t.cell) {
        vec![(cell.0 as u32, cell.1 as u32)]
    } else {
        (0..table.n_rows()).flat_map(|r| (0..table.n_cols()).map(move |c| (r as u32, c as u32))).collect()
    };
    if cells.is_empty() {
        return Err(LocalError::BadParams("table.cellFill names no cell".into()));
    }
    if cells.len() > MAX_FILL_CELLS {
        return Err(LocalError::BadParams(format!("table.cellFill names {} cells; at most {MAX_FILL_CELLS} in one step", cells.len())));
    }
    if let Some((r, c)) = cells.iter().find(|(r, c)| *r as usize >= table.n_rows() || *c as usize >= table.n_cols()) {
        return Err(LocalError::BadParams(format!("the table has no cell ({r}, {c})")));
    }
    Ok(cells)
}

/// Bytes for `insert.picture|insert.audio|insert.video|insert.model3d`: base64 `data`, or a local
/// `path` on desktop. The web build has no file system, so the host passes `data`.
/// One document-font operation from the user's command. The font file itself has already been
/// stored on the media channel (`submit` replaces `data`/`path` with the digest), so the
/// operation carries only the digest. A typeface is the document's key for the entry; a face is
/// one of its four slots. Nothing names a slide, because the entry belongs to the presentation.
fn font_payload(cmd: &str, p: &Value) -> Result<Payload, LocalError> {
    let typeface = p
        .get("typeface")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| LocalError::BadParams(format!("{cmd} needs `typeface`")))?;
    if typeface.chars().count() > 255 || typeface.chars().any(char::is_control) {
        return Err(LocalError::BadParams(format!("{cmd}: `typeface` is not a usable font name")));
    }
    let keys = |allowed: &[&str]| -> Result<(), LocalError> {
        for key in p.as_object().map(|o| o.keys()).into_iter().flatten() {
            if !allowed.contains(&key.as_str()) {
                return Err(LocalError::Unsupported(format!("{cmd} {key}")));
            }
        }
        Ok(())
    };
    match cmd {
        "font.create" => {
            keys(&["typeface", "face", "sha256", "name", "data", "path"])?;
            let face = match p.get("face").and_then(Value::as_str) {
                None => crate::FontFace::Regular,
                Some(name) => crate::FontFace::parse(name)
                    .ok_or_else(|| LocalError::BadParams(format!("font.create: `face` is regular, bold, italic or boldItalic, not `{name}`")))?,
            };
            let sha = p.get("sha256").and_then(Value::as_str).ok_or_else(|| LocalError::BadParams("font.create: the font file is missing".into()))?;
            if !crate::media::valid_sha(sha) {
                return Err(LocalError::BadParams("font.create: the font digest is not a sha256".into()));
            }
            let name = p.get("name").and_then(Value::as_str).unwrap_or("font").to_string();
            Ok(Payload::FontCreate { typeface: typeface.to_string(), face, sha256: sha.to_string(), name })
        }
        "font.set" => {
            keys(&["typeface", "panose", "pitchFamily", "charset"])?;
            let panose = match p.get("panose") {
                None | Some(Value::Null) => None,
                Some(value) => {
                    let text =
                        value.as_str().map(str::trim).ok_or_else(|| LocalError::BadParams("font.set: `panose` must be 20 hex digits".into()))?;
                    if text.len() != 20 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
                        return Err(LocalError::BadParams("font.set: `panose` must be 20 hex digits".into()));
                    }
                    Some(text.to_string())
                }
            };
            let field = |key: &str| -> Result<Option<u8>, LocalError> {
                match p.get(key) {
                    None | Some(Value::Null) => Ok(None),
                    Some(value) => value
                        .as_u64()
                        .and_then(|v| u8::try_from(v).ok())
                        .map(Some)
                        .ok_or_else(|| LocalError::BadParams(format!("font.set: `{key}` must be a number from 0 to 255"))),
                }
            };
            let pitch_family = field("pitchFamily")?;
            let charset = field("charset")?;
            if panose.is_none() && pitch_family.is_none() && charset.is_none() {
                return Err(LocalError::BadParams("font.set needs `panose`, `pitchFamily` or `charset`".into()));
            }
            Ok(Payload::FontSet { typeface: typeface.to_string(), panose, pitch_family, charset })
        }
        _ => {
            keys(&["typeface"])?;
            Ok(Payload::FontRemove { typeface: typeface.to_string() })
        }
    }
}

fn media_bytes(p: &Value, cmd: &str) -> Result<(String, Vec<u8>), LocalError> {
    let name = p.get("name").and_then(Value::as_str).unwrap_or("media").to_string();
    if let Some(data) = p.get("data").and_then(Value::as_str) {
        let bytes =
            base64::engine::general_purpose::STANDARD.decode(data).map_err(|_| LocalError::BadParams(format!("{cmd}: `data` is not base64")))?;
        return Ok((name, bytes));
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = p.get("path").and_then(Value::as_str) {
        let bytes = std::fs::read(path).map_err(|e| LocalError::BadParams(format!("{path}: {e}")))?;
        return Ok((name, bytes));
    }
    Err(LocalError::BadParams(format!("{cmd} needs `data` or `path`")))
}

/// A final fill from a `shape.fill` request. Only absolute forms are normalised: a solid colour
/// (string), `none: true` or `background: true`. Gradients, pictures, patterns and transparency
/// relative to an inherited style stay out of v1 (RFC §15).
fn fill_spec(p: &Value) -> Result<FillSpec, LocalError> {
    for key in p.as_object().map(|o| o.keys()).into_iter().flatten() {
        if !matches!(key.as_str(), "color" | "none" | "background" | "id" | "ids") {
            return Err(LocalError::Unsupported(format!("shape.fill {key}")));
        }
    }
    let color = p.get("color").and_then(Value::as_str);
    let none = p.get("none").and_then(Value::as_bool).unwrap_or(false);
    let background = p.get("background").and_then(Value::as_bool).unwrap_or(false);
    match (color, none, background) {
        (Some(color), false, false) => Ok(FillSpec::Solid { color: color.to_string() }),
        (None, true, false) => Ok(FillSpec::None),
        (None, false, true) => Ok(FillSpec::Background),
        (None, false, false) => Err(LocalError::BadParams("shape.fill needs `color`, `none` or `background`".into())),
        _ => Err(LocalError::BadParams("shape.fill takes one of `color`, `none` or `background`".into())),
    }
}

/// A final outline from a `shape.line` request. `reset` is not a value on the wire; a client that
/// wants the shape's default outline sends the explicit colour and width.
fn line_spec(p: &Value) -> Result<LineSpec, LocalError> {
    let mut spec = LineSpec::default();
    for (key, value) in p.as_object().map(|o| o.iter()).into_iter().flatten() {
        match key.as_str() {
            "none" => spec.none = value.as_bool().unwrap_or(false),
            "color" => {
                spec.color = Some(value.as_str().ok_or_else(|| LocalError::BadParams("shape.line color must be a string".into()))?.to_string())
            }
            "width" => {
                spec.width = Some(
                    value
                        .as_f64()
                        .filter(|w| w.is_finite() && *w >= 0.0)
                        .ok_or_else(|| LocalError::BadParams("shape.line width must be a number".into()))?,
                )
            }
            "dash" | "cap" | "join" | "compound" | "head" | "tail" | "headSize" | "tailSize" => {
                let text = value.as_str().ok_or_else(|| LocalError::BadParams(format!("shape.line {key} must be a string")))?;
                match key.as_str() {
                    "dash" => spec.dash = Some(text.to_string()),
                    "cap" => spec.cap = Some(text.to_string()),
                    "join" => spec.join = Some(text.to_string()),
                    "compound" => spec.compound = Some(text.to_string()),
                    "head" => spec.head = Some(text.to_string()),
                    "tail" => spec.tail = Some(text.to_string()),
                    "headSize" => spec.head_size = Some(text.to_string()),
                    _ => spec.tail_size = Some(text.to_string()),
                }
            }
            "id" | "ids" => {}
            other => return Err(LocalError::Unsupported(format!("shape.line {other}"))),
        }
    }
    if !spec.none
        && spec.color.is_none()
        && spec.width.is_none()
        && spec.dash.is_none()
        && spec.cap.is_none()
        && spec.join.is_none()
        && spec.compound.is_none()
        && spec.head.is_none()
        && spec.tail.is_none()
        && spec.head_size.is_none()
        && spec.tail_size.is_none()
    {
        return Err(LocalError::BadParams("shape.line needs `none` or at least one outline value".into()));
    }
    Ok(spec)
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

/// One allowlisted `format.*` command as wire operations, aimed at one body or at N cells of one
/// table: the whole body of a shape or of a cell, a character range inside one of those bodies,
/// or a caret (`RFC §15`).
///
/// A table cell has its own body. Its address comes from an explicit `cell` (or `row`+`col`),
/// else from the selected cells, else from the cell a text caret/range is in. A selection of
/// several cells becomes **N addresses, one per cell** — never one address widened to the whole
/// table — so the server formats exactly those cells and undo restores exactly their bodies. A
/// text range or a caret inside a shape or a cell becomes whole-body character offsets — the same
/// coordinates `text.insert`/`text.delete` use — so the server rebases it and every replica
/// derives the same runs. A caret in a shape's own body is an address too: the single-user
/// command (not the normaliser) decides which characters it touches.
fn format_ops(
    keys: &crate::KeyMap,
    st: &DocState,
    slide: &deckcraft_model::Slide,
    format: FormatCmd,
    p: &Value,
    make: &impl Fn(Target, Payload) -> OpEnvelope,
) -> Result<Vec<OpEnvelope>, LocalError> {
    let cmd = format.id();
    let slide_key = keys.slide_key(slide.id).cloned();
    let explicit_cell = cell_args(p);
    let mut args = p.clone();
    if let Some(object) = args.as_object_mut() {
        object.remove("ids");
        object.remove("id");
        object.remove("cell");
        object.remove("row");
        object.remove("col");
    }
    if !format.accepts(&args) {
        return Err(LocalError::BadParams(format!("{cmd} takes only its own arguments")));
    }
    if matches!(
        format,
        FormatCmd::Bold | FormatCmd::Italic | FormatCmd::Underline | FormatCmd::Strikethrough | FormatCmd::Superscript | FormatCmd::Subscript
    ) && args.get("on").and_then(Value::as_bool).is_none()
    {
        return Err(LocalError::BadParams(format!("{cmd} needs an explicit `on` in a shared session")));
    }
    let ids = shape_ids(p, st)?;
    if ids.is_empty() {
        return Err(LocalError::BadParams("formatting needs a shape".into()));
    }
    let mut out = Vec::new();
    for id in ids {
        let shape = find_shape(&slide.shapes, id).ok_or_else(|| LocalError::BadParams("no such shape".into()))?;
        let addresses = format_addresses(st, id, explicit_cell, shape)?;
        out.push(make(
            Target { slide: slide_key.clone(), shape: Some(shape_key(keys, slide.id, id)?), ..Target::default() },
            Payload::FormatSet { format, args: args.clone(), addresses },
        ));
    }
    Ok(out)
}

/// Which characters of one addressed body a local `format.*` command selects.
#[derive(Default)]
struct TextPart {
    range: Option<TextRange>,
    caret: Option<u32>,
}

/// Every body one local `format.*` command addresses in `shape`: one body for a shape, one
/// address per cell for a table (and N addresses when a range of cells is selected).
fn format_addresses(st: &DocState, id: ShapeId, explicit_cell: Option<(u32, u32)>, shape: &Shape) -> Result<Vec<FormatAddress>, LocalError> {
    match &shape.kind {
        ShapeKind::Table(t) => {
            if let Some(cell) = explicit_cell {
                let body = t
                    .cell(cell.0 as usize, cell.1 as usize)
                    .ok_or_else(|| LocalError::BadParams(format!("the table has no cell ({}, {})", cell.0, cell.1)))?;
                let part = selected_part(st, id, Some(cell), &body.text)?;
                return Ok(vec![FormatAddress { cell: Some(cell), range: part.range, caret: part.caret }]);
            }
            let text = st.selection.text.as_ref().filter(|t| t.shape == id && !t.notes);
            if let Some((_, _, _)) = st.selection.cells.filter(|(table, ..)| *table == id) {
                // A cell selection is N addresses, one per cell; a text range elsewhere would be
                // silently dropped, so it is a contradiction instead.
                if text.is_some_and(|t| t.is_range()) {
                    return Err(LocalError::BadParams("a cell range and a text range are not one address".into()));
                }
                let block = deckcraft_engine::cmd::table::selected_range(st.selection.cells, id, t);
                let mut addresses = Vec::new();
                for (r, row) in t.rows.iter().enumerate() {
                    for c in 0..row.cells.len() {
                        if deckcraft_engine::cmd::table::in_range(block, r, c) {
                            addresses.push(FormatAddress { cell: Some((r as u32, c as u32)), ..FormatAddress::default() });
                        }
                    }
                }
                if addresses.is_empty() {
                    return Err(LocalError::BadParams("the selected cell range addresses no cell".into()));
                }
                return Ok(addresses);
            }
            let Some(selection) = text.and_then(|t| t.cell) else {
                return Err(LocalError::Unsupported(format!("{} on a table needs a cell address", shape.id.0)));
            };
            let (r, c) = selection;
            let body = t.cell(r, c).ok_or_else(|| LocalError::BadParams(format!("the table has no cell ({r}, {c})")))?;
            let part = selected_part(st, id, Some((r as u32, c as u32)), &body.text)?;
            Ok(vec![FormatAddress { cell: Some((r as u32, c as u32)), range: part.range, caret: part.caret }])
        }
        _ => {
            if explicit_cell.is_some() {
                return Err(LocalError::BadParams("the shape is not a table".into()));
            }
            let body = shape.text.as_ref().ok_or_else(|| LocalError::Unsupported(format!("{} on a shape without text", shape.id.0)))?;
            let part = selected_part(st, id, None, body)?;
            Ok(vec![FormatAddress { cell: None, range: part.range, caret: part.caret }])
        }
    }
}

/// The `index` of one animation step in a local animation command (`None` when the command names
/// its steps by shape instead).
fn animation_index(p: &Value) -> Result<Option<usize>, LocalError> {
    match p.get("index") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .map(Some)
            .ok_or_else(|| LocalError::BadParams(format!("`index` is not a valid animation index: {value}"))),
    }
}

/// A finished gesture (a shape drag) names the boxes it left, so the room orders the result every
/// replica draws instead of one replica keeping the move to itself. The values are final: they are
/// checked, never recomputed from a replica's own state.
fn drag_bounds(
    keys: &crate::KeyMap,
    p: &Value,
    slide: &Slide,
    slide_key: Option<SlideKey>,
    op_id: &str,
    doc: &str,
) -> Result<Vec<OpEnvelope>, LocalError> {
    let items = p.get("items").and_then(Value::as_array).ok_or_else(|| LocalError::BadParams("shape.setBoundsMany needs `items`".into()))?;
    if items.len() > 4096 {
        return Err(LocalError::BadParams("too many boxes in one step".into()));
    }
    let mut boxes: Vec<ShapeBounds> = Vec::new();
    for item in items {
        let id = item
            .get("id")
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| LocalError::BadParams("a box without an id".into()))?;
        let shape = shape_key(keys, slide.id, ShapeId(id))?;
        if boxes.iter().any(|b| b.shape == shape) {
            return Err(LocalError::BadParams("a box names the same shape twice".into()));
        }
        let number = |name: &str| {
            item.get(name)
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite())
                .ok_or_else(|| LocalError::BadParams(format!("a box without a finite `{name}`")))
        };
        boxes.push(ShapeBounds { shape, x: number("x")?, y: number("y")?, w: number("w")?, h: number("h")? });
    }
    if boxes.is_empty() {
        return Ok(Vec::new());
    }
    let shapes = boxes.iter().map(|b| b.shape.clone()).collect();
    let payload = Payload::ShapesSetBounds { items: boxes };
    Ok(vec![OpEnvelope {
        op_id: op_id.to_string(),
        doc: doc.to_string(),
        base_version: 0,
        kind: class_of(&payload),
        target: Target { slide: slide_key, shapes, ..Target::default() },
        payload,
        history: None,
        preview: None,
    }])
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

/// The normalised operations of `animation.options`, `animation.clear` and
/// `animation.painterApply`: each leaves a slide's whole ordered animation sequence, so the wire
/// value is the same `slide_animations` the `animation.set`/`timing`/`move` family sends.
fn animation_ops(
    keys: &crate::KeyMap,
    st: &DocState,
    slide: &deckcraft_model::Slide,
    painter: Option<&(Vec<Animation>, bool)>,
    cmd: &str,
    p: &Value,
    make: &impl Fn(Target, Payload) -> OpEnvelope,
) -> Result<Vec<OpEnvelope>, LocalError> {
    if cmd == "animation.clear" {
        // `index` is a slide, not a step: the command clears one slide's whole sequence, and the
        // address is that slide, never the one the sender happens to be looking at.
        let at = match p.get("index") {
            None | Some(Value::Null) => st.selection.slide,
            Some(value) => value
                .as_u64()
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(|| LocalError::BadParams(format!("`index` is not a slide index: {value}")))?,
        };
        let cleared = st.doc.slides.get(at).ok_or_else(|| LocalError::BadParams("no such slide".into()))?;
        if cleared.animations.is_empty() {
            return Ok(Vec::new());
        }
        return animation_list_ops(keys, cleared, &[], make);
    }
    let animations = match cmd {
        "animation.options" => {
            let index = animation_index(p)?;
            let ids: Vec<ShapeId> = if index.is_none() { shape_ids(p, st)? } else { Vec::new() };
            let size = (st.doc.slide_size.width, st.doc.slide_size.height);
            deckcraft_engine::cmd::animation::options_plan(p, &slide.animations, index, &ids, size)
                .map_err(|e| LocalError::BadParams(e.to_string()))?
                .0
        }
        "animation.painterApply" => {
            // The clipboard is a session value, not a document value: the picked steps travel as
            // the slide's whole resulting sequence, computed here with the same plan the
            // single-user command runs, so every replica can apply it without a clipboard.
            let Some((picked, _)) = painter else {
                return Err(LocalError::BadParams("pick animations with the Animation Painter first".into()));
            };
            let ids = shape_ids(p, st)?;
            if ids.is_empty() {
                return Err(LocalError::BadParams("no target shapes selected".into()));
            }
            for id in &ids {
                find_shape(&slide.shapes, *id).ok_or_else(|| LocalError::BadParams("no such shape".into()))?;
            }
            deckcraft_engine::cmd::animation::painter_plan(picked, &slide.animations, &ids, |id| {
                deckcraft_engine::cmd::animation::paragraphs(&slide.shapes, id)
            })
        }
        other => return Err(LocalError::Unsupported(other.into())),
    };
    if animations == slide.animations {
        return Ok(Vec::new());
    }
    animation_list_ops(keys, slide, &animations, make)
}

/// One operation carrying a slide's whole ordered animation list, the normalised form of
/// `animation.set`, `animation.timing` and `animation.move`. The bounds are the engine
/// command's own, so a list the server would refuse is refused before anything is sent.
fn animation_list_ops(
    keys: &crate::KeyMap,
    slide: &deckcraft_model::Slide,
    animations: &[Animation],
    make: &impl Fn(Target, Payload) -> OpEnvelope,
) -> Result<Vec<OpEnvelope>, LocalError> {
    if animations.len() > deckcraft_engine::cmd::animation::MAX_SLIDE_ANIMATION_STEPS {
        return Err(LocalError::BadParams("too many animation steps on one slide".into()));
    }
    let values = animations
        .iter()
        .map(crate::animation::spec)
        .collect::<Option<Vec<Value>>>()
        .ok_or_else(|| LocalError::BadParams("the animation steps cannot be sent".into()))?;
    let bytes = serde_json::to_vec(&values).map_err(|e| LocalError::BadParams(format!("the animation list cannot be sent: {e}")))?;
    if bytes.len() > deckcraft_engine::cmd::animation::MAX_SLIDE_ANIMATION_JSON {
        return Err(LocalError::BadParams("the animation list is too large to send".into()));
    }
    let key = keys.slide_key(slide.id).cloned().ok_or(LocalError::NoDocument)?;
    Ok(vec![make(Target { slide: Some(key), ..Target::default() }, Payload::SlideAnimations { animations: values })])
}

/// A cell address a local command carries in its arguments: `cell: [row, column]`, or `row` and
/// `col`. Anything else is no address at all.
fn cell_args(p: &Value) -> Option<(u32, u32)> {
    let pair = p
        .get("cell")
        .and_then(Value::as_array)
        .and_then(|a| Some((u32::try_from(a.first()?.as_u64()?).ok()?, u32::try_from(a.get(1)?.as_u64()?).ok()?)));
    pair.or_else(|| Some((u32::try_from(p.get("row")?.as_u64()?).ok()?, u32::try_from(p.get("col")?.as_u64()?).ok()?)))
}

/// The characters a local `format.*` command addresses inside one body: a character range when
/// the text selection is a range in that body, a caret when it is collapsed in a shape's own
/// body, and nothing when the whole body (or a whole cell) is addressed.
///
/// The selection is authoritative only for the body it edits: a range in another cell or shape
/// is not silently moved to this one. A collapsed caret in a table cell addresses that whole
/// cell. A caret in a shape's own body is an address of its own — the engine's single-user
/// command decides which characters it touches (a run command widens it to the word it is in,
/// a paragraph command takes the paragraph), and every replica runs that same command against
/// the same authoritative body, so no replica has to guess (RFC §15).
fn selected_part(st: &DocState, id: ShapeId, cell: Option<(u32, u32)>, body: &deckcraft_model::text::TextBody) -> Result<TextPart, LocalError> {
    let Some(text) = st.selection.text.as_ref().filter(|t| t.shape == id && !t.notes) else { return Ok(TextPart::default()) };
    let selection_cell = text.cell.map(|(r, c)| (r as u32, c as u32));
    if selection_cell != cell {
        if text.is_range() {
            return Err(LocalError::BadParams("the selected range is not in the addressed body".into()));
        }
        return Ok(TextPart::default());
    }
    if !text.is_range() {
        if cell.is_some() {
            return Ok(TextPart::default());
        }
        let at = position_offset(Some(body), text.ordered().0);
        return Ok(TextPart { range: None, caret: Some(at as u32) });
    }
    let (a, b) = text.ordered();
    let (from, to) = (position_offset(Some(body), a), position_offset(Some(body), b));
    if to <= from {
        return Err(LocalError::BadParams("nothing selected".into()));
    }
    Ok(TextPart { range: Some(TextRange { from: from as u32, to: to as u32 }), caret: None })
}

/// The slide and whole body a `slide.notes` edit leaves behind. `body` is the formatted body the
/// notes editor holds; `text` is applied to the current body with the same `replace_text` the
/// single-user command runs, so the value that travels is always the complete body.
fn notes_body(keys: &crate::KeyMap, st: &DocState, p: &Value) -> Result<(SlideKey, Value), LocalError> {
    let index = match p.get("index") {
        Some(value) => usize::try_from(value.as_u64().ok_or_else(|| LocalError::BadParams("slide.notes: `index` must be a slide number".into()))?)
            .map_err(|_| LocalError::BadParams("slide.notes: `index` must be a slide number".into()))?,
        None => st.selection.slide,
    };
    let target = st.doc.slides.get(index).ok_or_else(|| LocalError::BadParams("slide.notes: no such slide".into()))?;
    let key = keys.slide_key(target.id).cloned().ok_or(LocalError::NoDocument)?;
    let notes = match p.get("body") {
        Some(body) => serde_json::from_value::<deckcraft_model::text::TextBody>(body.clone())
            .map_err(|e| LocalError::BadParams(format!("slide.notes: invalid `body`: {e}")))?,
        None => {
            let text = p.get("text").and_then(Value::as_str).ok_or_else(|| LocalError::BadParams("slide.notes needs `text` or `body`".into()))?;
            let mut body = target.notes.clone();
            deckcraft_model::edit::replace_text(&mut body, text);
            body
        }
    };
    let value = serde_json::to_value(&notes).map_err(|_| LocalError::BadParams("slide.notes: the body cannot be sent".into()))?;
    Ok((key, value))
}

/// The text body a local text command edits, and the table cell it lives in. A table cell has
/// its own body; a table without a cell address is refused, and the offset is inside that body.
fn selected_body<'a>(
    shapes: &'a [Shape],
    ts: &deckcraft_engine::TextSel,
) -> Result<(Option<&'a deckcraft_model::text::TextBody>, Option<(u32, u32)>), LocalError> {
    let shape = find_shape(shapes, ts.shape).ok_or_else(|| LocalError::BadParams("no such shape".into()))?;
    match (&shape.kind, ts.cell) {
        (ShapeKind::Table(t), Some((r, c))) => {
            let cell = t.cell(r, c).ok_or_else(|| LocalError::BadParams(format!("the table has no cell ({r}, {c})")))?;
            Ok((Some(&cell.text), Some((r as u32, c as u32))))
        }
        (ShapeKind::Table(_), None) => Err(LocalError::Unsupported("a table text command needs a cell".into())),
        _ => Ok((shape.text.as_ref(), None)),
    }
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
    matches!(payload, Payload::ShapeMove { .. } | Payload::ShapeSetBounds { .. } | Payload::ShapesSetBounds { .. } | Payload::TextSet { .. })
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
    use deckcraft_collab_proto::Role;
    use deckcraft_engine::{Session, TextSel};
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

    /// One slide with a 2×2 table; its top-left cell holds "ab".
    fn table_deck() -> (Session, String, SlideId, ShapeId) {
        let mut s = Session::with_new();
        let title = s.doc().expect("doc").doc.slides[0].id;
        s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("blank");
        s.apply_remote("slide.delete", &json!({"ids": [title.0]})).expect("drop title");
        s.apply_remote("insert.table", &json!({"rows": 2, "cols": 2, "rect": [100, 100, 300, 100]})).expect("table");
        let st = s.doc().expect("doc");
        let slide = st.doc.slides[0].id;
        let table = st.doc.slides[0].shapes[0].id;
        s.apply_remote("text.set", &json!({"id": table.0, "cell": [0, 0], "text": "ab"})).expect("seed");
        (s, "d".into(), slide, table)
    }

    fn caret(s: &mut Session, shape: ShapeId, at: (usize, usize), cell: Option<(usize, usize)>) {
        s.doc_mut().expect("doc").selection.text = Some(TextSel { shape, anchor: at, caret: at, cell, notes: false, line_end: None });
    }

    /// A whole-cell format address.
    fn cell(at: (u32, u32)) -> FormatAddress {
        FormatAddress { cell: Some(at), ..FormatAddress::default() }
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

    /// A command that names several shapes is several operations on the wire, so each one needs
    /// its own identity: the room answers a repeated `opId` with the stored result and would
    /// silently drop the rest (ADR 0003). The first keeps the id the caller handed in, which is
    /// what the optimistic preview and the sender's own settle are keyed on.
    #[test]
    fn a_move_of_a_multiple_selection_gives_each_operation_its_own_id() {
        let (s, doc, slide, shapes) = deck();
        let km = keys(s.doc().expect("doc").doc.as_ref());
        let st = s.doc().expect("doc");
        let p = json!({"ids": [shapes[0].0, shapes[1].0], "dx": 25.0, "dy": -5.0});
        let ops = normalize(&km, st, "shape.move", &p, "op", &doc).expect("normalise");
        assert_eq!(ops.len(), 2);
        assert_eq!(ops.iter().map(|op| op.op_id.as_str()).collect::<Vec<_>>(), vec!["op", "op#1"], "one identity per operation");
        for (op, id) in ops.iter().zip([shapes[0], shapes[1]]) {
            assert_eq!(op.target.slide.as_ref().and_then(|k| km.slide_id(k)), Some(slide));
            assert_eq!(op.target.shape.as_ref().and_then(|k| km.shape_id(slide, k)), Some(id));
        }
        assert_eq!(ops[0].payload, Payload::ShapeMove { x: 125.0, y: 95.0 });
        assert_eq!(ops[1].payload, Payload::ShapeMove { x: 425.0, y: 95.0 });
        // The same command normalised again gives the same ids, so a resend is still one
        // operation per shape and the room dedups it instead of applying it twice.
        let again = normalize(&km, st, "shape.move", &p, "op", &doc).expect("normalise again");
        assert_eq!(again.iter().map(|op| op.op_id.as_str()).collect::<Vec<_>>(), vec!["op", "op#1"]);
    }

    #[test]
    fn a_resize_of_a_multiple_selection_gives_each_operation_its_own_id() {
        let (s, doc, slide, shapes) = deck();
        let km = keys(s.doc().expect("doc").doc.as_ref());
        let st = s.doc().expect("doc");
        let p = json!({"ids": [shapes[0].0, shapes[1].0], "x": 10.0, "y": 20.0, "w": 30.0, "h": 40.0});
        let ops = normalize(&km, st, "shape.setBounds", &p, "op", &doc).expect("normalise");
        assert_eq!(ops.len(), 2);
        assert_eq!(ops.iter().map(|op| op.op_id.as_str()).collect::<Vec<_>>(), vec!["op", "op#1"]);
        for (op, id) in ops.iter().zip([shapes[0], shapes[1]]) {
            assert_eq!(op.payload, Payload::ShapeSetBounds { x: 10.0, y: 20.0, w: 30.0, h: 40.0 });
            assert_eq!(op.target.shape.as_ref().and_then(|k| km.shape_id(slide, k)), Some(id));
        }
    }

    #[test]
    fn a_finished_drag_normalises_to_one_operation_with_its_final_boxes() {
        let (s, doc, slide, shapes) = deck();
        let km = keys(s.doc().expect("doc").doc.as_ref());
        // Two objects as a drag leaves them: the boxes are final, so nothing is recomputed here.
        let p = json!({"items": [
            {"id": shapes[0].0, "x": 140.0, "y": 120.0, "w": 100.0, "h": 50.0},
            {"id": shapes[1].0, "x": 260.0, "y": 120.0, "w": 100.0, "h": 50.0},
        ]});
        let ops = normalize(&km, s.doc().expect("doc"), "shape.setBoundsMany", &p, "op", &doc).expect("normalise");
        assert_eq!(ops.len(), 1, "a drag of two objects is one version and one undo step");
        assert_eq!(ops[0].kind, OpKind::Command);
        assert_eq!(ops[0].target.slide.as_ref().and_then(|k| km.slide_id(k)), Some(slide));
        match &ops[0].payload {
            Payload::ShapesSetBounds { items } => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].shape, km.shape_key(slide, shapes[0]).expect("key").clone());
                assert_eq!((items[0].x, items[0].y, items[0].w, items[0].h), (140.0, 120.0, 100.0, 50.0));
                assert_eq!((items[1].x, items[1].y), (260.0, 120.0));
            }
            other => panic!("{other:?}"),
        }
        // A box without an id, a repeated shape and an empty list: the first two are refused
        // before anything is sent, the empty one sends nothing at all.
        let bad = json!({"items": [{"x": 1.0, "y": 1.0, "w": 10.0, "h": 10.0}]});
        assert!(normalize(&km, s.doc().expect("doc"), "shape.setBoundsMany", &bad, "op", &doc).is_err());
        let twice = json!({"items": [
            {"id": shapes[0].0, "x": 1.0, "y": 1.0, "w": 10.0, "h": 10.0},
            {"id": shapes[0].0, "x": 2.0, "y": 2.0, "w": 10.0, "h": 10.0},
        ]});
        assert!(normalize(&km, s.doc().expect("doc"), "shape.setBoundsMany", &twice, "op", &doc).is_err());
        let empty = json!({"items": []});
        assert!(normalize(&km, s.doc().expect("doc"), "shape.setBoundsMany", &empty, "op", &doc).expect("no-op").is_empty());
    }

    #[test]
    fn a_transition_command_normalises_to_its_whole_final_value() {
        let (mut s, doc, slide, _) = deck();
        s.execute("transition.timing", &json!({"after": 4000})).expect("timing");
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());
        let ops = normalize(&km, st, "transition.set", &json!({"kind": "push", "option": "l"}), "op", &doc).expect("transition");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].kind, OpKind::Command);
        assert_eq!(ops[0].target.slide.as_ref().and_then(|k| km.slide_id(k)), Some(slide));
        match &ops[0].payload {
            Payload::SlideTransition { transition: Some(value) } => {
                assert_eq!((value.kind.as_str(), value.option.as_str(), value.duration_ms), ("push", "l", 1000));
                assert_eq!(value.advance_after_ms, Some(4000), "the plan keeps what the user did not name");
                assert!(value.advance_on_click);
            }
            other => panic!("{other:?}"),
        }

        // `none` with nothing else to keep collapses to the model's "no transition".
        s.execute("transition.timing", &json!({"after": null})).expect("clear the timing");
        let st = s.doc().expect("doc");
        let ops = normalize(&km, st, "transition.set", &json!({"kind": "none"}), "op", &doc).expect("none");
        assert!(matches!(ops[0].payload, Payload::SlideTransition { transition: None }));

        // An unknown effect is refused before anything is sent, and a bad index never falls back
        // to the selected slide.
        assert!(matches!(normalize(&km, st, "transition.set", &json!({"kind": "nope"}), "op", &doc), Err(LocalError::BadParams(_))));
        assert!(matches!(normalize(&km, st, "transition.set", &json!({"kind": "fade", "index": -1}), "op", &doc), Err(LocalError::BadParams(_))));
    }

    #[test]
    fn an_animation_command_normalises_to_the_shapes_whole_step_list() {
        let (mut s, doc, slide, shapes) = deck();
        s.execute("animation.add", &json!({"effect": "fade", "ids": [shapes[0].0]})).expect("seed fade");
        s.execute("animation.add", &json!({"effect": "zoom", "ids": [shapes[1].0]})).expect("seed zoom");
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());

        let ops =
            normalize(&km, st, "animation.add", &json!({"effect": "pulse", "class": "emphasis", "ids": [shapes[0].0]}), "op", &doc).expect("add");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].kind, OpKind::Command);
        assert_eq!(ops[0].target.slide.as_ref().and_then(|k| km.slide_id(k)), Some(slide));
        assert_eq!(ops[0].target.shape.as_ref().and_then(|k| km.shape_id(slide, k)), Some(shapes[0]));
        let Payload::ShapeAnimations { animations } = &ops[0].payload else { panic!("{:?}", ops[0].payload) };
        let effects: Vec<&str> = animations.iter().filter_map(|a| a.get("effect").and_then(Value::as_str)).collect();
        assert_eq!(effects, ["fade", "pulse"], "the shape's own steps plus the new one");
        assert!(
            animations.iter().all(|a| a.get("shape").and_then(Value::as_u64) == Some(shapes[0].0 as u64)),
            "every step names the addressed shape"
        );

        // `animation.remove` by index sends that shape's list without the named step; the address
        // is the step's shape, not whatever happens to be selected.
        let ops = normalize(&km, st, "animation.remove", &json!({"index": 1}), "op", &doc).expect("remove");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].target.shape.as_ref().and_then(|k| km.shape_id(slide, k)), Some(shapes[1]));
        let Payload::ShapeAnimations { animations } = &ops[0].payload else { panic!("{:?}", ops[0].payload) };
        assert!(animations.is_empty(), "the shape had one step and it is the one removed");

        // A shape without a step has nothing to remove, so nothing is sent.
        let (bare, bare_doc, _, bare_shapes) = deck();
        let bare_st = bare.doc().expect("doc");
        let bare_keys = keys(bare_st.doc.as_ref());
        let ops = normalize(&bare_keys, bare_st, "animation.remove", &json!({"ids": [bare_shapes[0].0]}), "op", &bare_doc).expect("no-op");
        assert!(ops.is_empty(), "a no-op sends nothing");
        // An unknown effect, an out-of-range index and a selection of several shapes are refused
        // before anything is sent.
        assert!(matches!(
            normalize(&km, st, "animation.add", &json!({"effect": "nope", "ids": [shapes[0].0]}), "op", &doc),
            Err(LocalError::BadParams(_))
        ));
        assert!(matches!(normalize(&km, st, "animation.remove", &json!({"index": 9}), "op", &doc), Err(LocalError::BadParams(_))));
        assert!(matches!(
            normalize(&km, st, "animation.add", &json!({"effect": "fade", "ids": [shapes[0].0, shapes[1].0]}), "op", &doc),
            Err(LocalError::Unsupported(_))
        ));
    }

    #[test]
    fn animation_set_timing_and_move_normalise_to_the_slides_whole_sequence() {
        let (mut s, doc, slide, shapes) = deck();
        s.execute("animation.add", &json!({"effect": "fade", "ids": [shapes[0].0]})).expect("seed fade");
        s.execute("animation.add", &json!({"effect": "zoom", "ids": [shapes[1].0]})).expect("seed zoom");
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());
        let effects = |payload: &Payload| match payload {
            Payload::SlideAnimations { animations } => {
                animations.iter().filter_map(|a| a.get("effect").and_then(Value::as_str)).map(String::from).collect::<Vec<_>>()
            }
            other => panic!("{other:?}"),
        };

        // `animation.set` replaces one shape's steps in place and sends the slide's whole sequence.
        let ops =
            normalize(&km, st, "animation.set", &json!({"effect": "pulse", "class": "emphasis", "ids": [shapes[0].0]}), "op", &doc).expect("set");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].target.slide.as_ref().and_then(|k| km.slide_id(k)), Some(slide));
        assert!(ops[0].target.shape.is_none(), "a slide-wide list is not a shape's address");
        assert_eq!(effects(&ops[0].payload), ["pulse".to_string(), "zoom".to_string()]);
        let Payload::SlideAnimations { animations } = &ops[0].payload else { panic!() };
        assert_eq!(animations[0].get("shape").and_then(Value::as_u64), Some(shapes[0].0 as u64));

        // `animation.timing` by index changes exactly that step, order untouched.
        let ops =
            normalize(&km, st, "animation.timing", &json!({"index": 0, "duration": 1_200, "start": "afterPrevious"}), "op", &doc).expect("timing");
        assert_eq!(effects(&ops[0].payload), ["fade".to_string(), "zoom".to_string()]);
        let Payload::SlideAnimations { animations } = &ops[0].payload else { panic!() };
        assert_eq!(animations[0].get("durationMs").and_then(Value::as_u64), Some(1_200));
        assert_eq!(animations[1].get("durationMs").and_then(Value::as_u64), Some(500), "the other step keeps its timing");

        // `animation.move` sends the reordered sequence, including across shapes.
        let ops = normalize(&km, st, "animation.move", &json!({"index": 0, "to": 1}), "op", &doc).expect("move");
        assert_eq!(effects(&ops[0].payload), ["zoom".to_string(), "fade".to_string()]);
        // A move that changes nothing sends nothing.
        let ops = normalize(&km, st, "animation.move", &json!({"index": 0, "to": 0}), "op", &doc).expect("no-op move");
        assert!(ops.is_empty());

        // Unknown effects, bad indices and a timing edit that names no step are refused.
        assert!(matches!(
            normalize(&km, st, "animation.set", &json!({"effect": "nope", "ids": [shapes[0].0]}), "op", &doc),
            Err(LocalError::BadParams(_))
        ));
        assert!(matches!(normalize(&km, st, "animation.move", &json!({"index": 9, "to": 0}), "op", &doc), Err(LocalError::BadParams(_))));
        assert!(matches!(normalize(&km, st, "animation.move", &json!({"index": 0}), "op", &doc), Err(LocalError::BadParams(_))));
        assert!(matches!(normalize(&km, st, "animation.timing", &json!({"index": 9, "duration": 10}), "op", &doc), Err(LocalError::BadParams(_))));
        // A timing edit that names no animated shape is refused too.
        let (bare, bare_doc, _, bare_shapes) = deck();
        let bare_st = bare.doc().expect("doc");
        let bare_keys = keys(bare_st.doc.as_ref());
        assert!(matches!(
            normalize(&bare_keys, bare_st, "animation.timing", &json!({"ids": [bare_shapes[0].0], "duration": 10}), "op", &bare_doc),
            Err(LocalError::BadParams(_))
        ));
    }

    #[test]
    fn animation_options_clear_and_the_painter_normalise_to_the_slides_whole_sequence() {
        let (mut s, doc, slide, shapes) = deck();
        s.execute("animation.add", &json!({"effect": "fade", "ids": [shapes[0].0]})).expect("seed fade");
        s.execute("animation.add", &json!({"effect": "zoom", "ids": [shapes[1].0]})).expect("seed zoom");
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());
        let effects = |payload: &Payload| match payload {
            Payload::SlideAnimations { animations } => {
                animations.iter().filter_map(|a| a.get("effect").and_then(Value::as_str)).map(String::from).collect::<Vec<_>>()
            }
            other => panic!("{other:?}"),
        };

        // `animation.options` sends the slide's whole sequence with the option the engine resolves
        // for the named step; the other step keeps its own.
        let ops = normalize(&km, st, "animation.options", &json!({"index": 0, "option": "fromBottom", "textBuild": "byParagraph"}), "op", &doc)
            .expect("options");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].target.slide.as_ref().and_then(|k| km.slide_id(k)), Some(slide));
        assert!(ops[0].target.shape.is_none(), "a slide-wide list is not a shape's address");
        assert_eq!(effects(&ops[0].payload), ["fade".to_string(), "zoom".to_string()]);
        let Payload::SlideAnimations { animations } = &ops[0].payload else { panic!() };
        assert_eq!(animations[0].get("textBuild").and_then(Value::as_str), Some("byParagraph"));
        assert_eq!(animations[1].get("textBuild").and_then(Value::as_str), Some("asOne"), "the other step keeps its build mode");
        // An options edit that changes nothing sends nothing; one that names no step is refused.
        assert!(normalize(&km, st, "animation.options", &json!({"index": 0}), "op", &doc).expect("no-op options").is_empty());
        assert!(matches!(normalize(&km, st, "animation.options", &json!({"index": 9, "option": "b"}), "op", &doc), Err(LocalError::BadParams(_))));

        // `animation.clear` addresses the slide it clears and sends an empty sequence.
        let ops = normalize(&km, st, "animation.clear", &json!({"index": 0}), "op", &doc).expect("clear");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].target.slide.as_ref().and_then(|k| km.slide_id(k)), Some(slide));
        assert!(effects(&ops[0].payload).is_empty(), "the slide's sequence is empty");
        // A slide with no steps has nothing to clear.
        let (bare, bare_doc, _, _) = deck();
        let bare_st = bare.doc().expect("doc");
        let bare_keys = keys(bare_st.doc.as_ref());
        assert!(normalize(&bare_keys, bare_st, "animation.clear", &json!({"index": 0}), "op", &bare_doc).expect("no-op clear").is_empty());
        assert!(matches!(normalize(&km, st, "animation.clear", &json!({"index": 9}), "op", &doc), Err(LocalError::BadParams(_))));

        // The painter: the picked steps travel as the slide's resulting sequence, so the target's
        // own step is replaced and no replica needs the clipboard.
        let picked: Vec<Animation> = st.doc.slides[0].animations.iter().filter(|a| a.shape == shapes[0]).cloned().collect();
        let clipboard = (picked.clone(), false);
        let ops = normalize_with_painter(&km, st, Some(&clipboard), "animation.painterApply", &json!({"ids": [shapes[1].0]}), "op", &doc)
            .expect("painterApply");
        assert_eq!(ops.len(), 1);
        assert_eq!(effects(&ops[0].payload), ["fade".to_string(), "fade".to_string()], "the target's zoom became the picked fade");
        let Payload::SlideAnimations { animations } = &ops[0].payload else { panic!() };
        assert_eq!(animations[1].get("shape").and_then(Value::as_u64), Some(shapes[1].0 as u64));
        // Without a clipboard, with no target and with an unknown target nothing is sent.
        assert!(matches!(normalize(&km, st, "animation.painterApply", &json!({"ids": [shapes[1].0]}), "op", &doc), Err(LocalError::BadParams(_))));
        assert!(matches!(
            normalize_with_painter(&km, st, Some(&clipboard), "animation.painterApply", &json!({"ids": []}), "op", &doc),
            Err(LocalError::BadParams(_))
        ));
        assert!(matches!(
            normalize_with_painter(&km, st, Some(&clipboard), "animation.painterApply", &json!({"ids": [9_999]}), "op", &doc),
            Err(LocalError::BadParams(_))
        ));
    }

    #[test]
    fn align_normalises_the_whole_selection_into_one_final_bounds_step() {
        let (mut s, doc, slide, shapes) = deck();
        s.execute("edit.select", &json!({"ids": shapes})).expect("select both");
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());
        let ops = normalize(&km, st, "arrange.align", &json!({"edge": "center"}), "op", &doc).expect("align");
        assert_eq!(ops.len(), 1, "one operation for the whole selection");
        assert_eq!(ops[0].kind, OpKind::Command);
        match &ops[0].payload {
            Payload::ShapesSetBounds { items } => {
                assert_eq!(items.len(), 2, "both objects move to the selection's centre");
                assert_eq!(items[0].shape, km.shape_key(slide, shapes[0]).expect("key").clone());
                assert_eq!((items[0].x, items[0].y), (210.0, 100.0));
                assert_eq!(items[1].shape, km.shape_key(slide, shapes[1]).expect("key").clone());
                assert_eq!((items[1].x, items[1].y), (250.0, 100.0));
                assert_eq!((items[0].w, items[0].h), (200.0, 100.0), "the size is not recomputed on the wire");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(ops[0].target.slide.as_ref().and_then(|k| km.slide_id(k)), Some(slide));
        assert_eq!(ops[0].target.shapes.len(), 2, "the address lists every moved shape");
    }

    #[test]
    fn distribute_normalises_gaps_into_one_final_bounds_step_and_skips_a_no_op() {
        let (mut s, doc, _, shapes) = deck();
        s.execute("edit.select", &json!({"ids": shapes})).expect("select both");
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());
        let ops = normalize(&km, st, "arrange.distribute", &json!({"dir": "horizontal"}), "op", &doc).expect("distribute");
        assert_eq!(ops.len(), 1);
        match &ops[0].payload {
            Payload::ShapesSetBounds { items } => {
                // Two objects against the slide: even gaps from 0 to the 960 pt width.
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].x, 0.0);
                assert_eq!(items[1].x, 840.0);
            }
            other => panic!("{other:?}"),
        }

        // Already aligned left: the step would change nothing, so nothing is sent.
        s.execute("arrange.align", &json!({"edge": "left"})).expect("align left in the single-user engine");
        let st = s.doc().expect("doc");
        let ops = normalize(&km, st, "arrange.align", &json!({"edge": "left"}), "op", &doc).expect("no-op align");
        assert!(ops.is_empty(), "an operation that moves nothing is not sent");
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
    fn deleting_several_slides_gives_each_operation_its_own_id() {
        let (mut s, doc, _, _) = deck();
        s.execute("slide.new", &json!({"layout": "blank"})).expect("second");
        s.execute("slide.new", &json!({"layout": "blank"})).expect("third");
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());
        let ids: Vec<u64> = st.doc.slides.iter().map(|slide| u64::from(slide.id.0)).collect();
        let ops = normalize(&km, st, "slide.delete", &json!({"ids": ids}), "op", &doc).expect("normalise");
        assert_eq!(ops.len(), 3);
        assert_eq!(
            ops.iter().map(|op| op.op_id.as_str()).collect::<Vec<_>>(),
            vec!["op", "op#1", "op#2"],
            "three deletions are three operations, not one answered three times"
        );
        for (op, id) in ops.iter().zip(&ids) {
            assert!(matches!(op.payload, Payload::SlideDelete {}));
            assert_eq!(op.kind, OpKind::Structural);
            assert_eq!(op.target.slide.as_ref().and_then(|k| km.slide_id(k)), Some(SlideId(*id as u32)));
        }
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
    fn typing_in_a_table_cell_normalises_with_its_address() {
        let (mut s, doc_id, _slide, table) = table_deck();
        caret(&mut s, table, (0, 1), Some((0, 0)));
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());

        let ops = normalize(&km, st, "text.insert", &json!({"text": "X"}), "op", &doc_id).expect("insert");
        assert_eq!(ops[0].payload, Payload::TextInsert { at: 1, text: "X".into(), cell: Some((0, 0)) });

        // A selected range in the cell becomes a delete inside the same body.
        caret(&mut s, table, (0, 2), Some((0, 0)));
        s.doc_mut().expect("doc").selection.text.as_mut().expect("caret").anchor = (0, 0);
        let st = s.doc().expect("doc");
        let ops = normalize(&km, st, "text.delete", &json!({}), "op", &doc_id).expect("delete");
        assert_eq!(ops[0].payload, Payload::TextDelete { from: 0, to: 2, cell: Some((0, 0)) });

        let ops = normalize(&km, st, "edit.delete", &json!({}), "op", &doc_id).expect("edit.delete");
        assert_eq!(ops[0].payload, Payload::TextDelete { from: 0, to: 2, cell: Some((0, 0)) });

        // `text.set` without an explicit `cell` takes it from the caret.
        let ops = normalize(&km, st, "text.set", &json!({"text": "new"}), "op", &doc_id).expect("set");
        assert_eq!(ops[0].payload, Payload::TextSet { text: "new".into(), cell: Some((0, 0)) });
    }

    #[test]
    fn formatting_a_table_cell_normalises_with_its_address() {
        let (mut s, doc_id, _slide, table) = table_deck();
        // One cell selected: the address comes from the selection.
        s.doc_mut().expect("doc").selection.shapes = vec![table];
        s.doc_mut().expect("doc").selection.cells = Some((table, (0, 0), (0, 0)));
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());
        let ops = normalize(&km, st, "format.bold", &json!({"on": true}), "op", &doc_id).expect("selected cell");
        assert_eq!(ops[0].payload, Payload::FormatSet { format: FormatCmd::Bold, args: json!({"on": true}), addresses: vec![cell((0, 0))] });

        // A collapsed caret inside a cell formats that cell's whole body.
        s.doc_mut().expect("doc").selection.cells = None;
        caret(&mut s, table, (0, 1), Some((0, 1)));
        let st = s.doc().expect("doc");
        let ops = normalize(&km, st, "format.italic", &json!({"on": true}), "op", &doc_id).expect("caret in a cell");
        assert_eq!(ops[0].payload, Payload::FormatSet { format: FormatCmd::Italic, args: json!({"on": true}), addresses: vec![cell((0, 1))] });

        // A range inside the cell is a run range in that cell's own body: the address carries
        // both the cell and whole-cell character offsets.
        caret(&mut s, table, (0, 0), Some((0, 0)));
        s.doc_mut().expect("doc").selection.text.as_mut().expect("caret").caret = (0, 1);
        let st = s.doc().expect("doc");
        let ops = normalize(&km, st, "format.italic", &json!({"on": true}), "op", &doc_id).expect("a range in a cell");
        assert_eq!(
            ops[0].payload,
            Payload::FormatSet {
                format: FormatCmd::Italic,
                args: json!({"on": true}),
                addresses: vec![FormatAddress { cell: Some((0, 0)), range: Some(TextRange { from: 0, to: 1 }), caret: None }]
            }
        );

        // An explicit address in another cell than a selected range is a contradiction: the
        // range would format characters of a body the command does not address.
        let st = s.doc().expect("doc");
        let e = normalize(&km, st, "format.bold", &json!({"on": true, "cell": [1, 1]}), "op", &doc_id).expect_err("range in another cell");
        assert!(matches!(e, LocalError::BadParams(_)), "{e:?}");

        // With a caret (no range) the explicit address wins over the selection and never travels
        // in the formatting args.
        s.doc_mut().expect("doc").selection.text = None;
        caret(&mut s, table, (0, 1), Some((0, 1)));
        let st = s.doc().expect("doc");
        let ops = normalize(&km, st, "format.bold", &json!({"on": true, "cell": [1, 1]}), "op", &doc_id).expect("explicit cell");
        assert_eq!(ops[0].payload, Payload::FormatSet { format: FormatCmd::Bold, args: json!({"on": true}), addresses: vec![cell((1, 1))] });

        // A range of cells is N addresses, one per cell: the operation names exactly the cells
        // it formats and never widens to the whole table.
        s.doc_mut().expect("doc").selection.cells = Some((table, (0, 0), (1, 1)));
        let st = s.doc().expect("doc");
        let ops = normalize(&km, st, "format.bold", &json!({"on": true}), "op", &doc_id).expect("a cell range");
        assert_eq!(
            ops[0].payload,
            Payload::FormatSet {
                format: FormatCmd::Bold,
                args: json!({"on": true}),
                addresses: vec![cell((0, 0)), cell((0, 1)), cell((1, 0)), cell((1, 1))]
            }
        );

        // A cell the table does not have is refused here as well.
        s.doc_mut().expect("doc").selection.cells = None;
        let st = s.doc().expect("doc");
        let e = normalize(&km, st, "format.bold", &json!({"on": true, "cell": [9, 9]}), "op", &doc_id).expect_err("no such cell");
        assert!(matches!(e, LocalError::BadParams(_)), "{e:?}");

        // A table without any address has no body to format.
        s.doc_mut().expect("doc").selection.text = None;
        let st = s.doc().expect("doc");
        let e = normalize(&km, st, "format.bold", &json!({"on": true}), "op", &doc_id).expect_err("a table needs a cell");
        assert!(matches!(e, LocalError::Unsupported(_)), "{e:?}");
    }

    #[test]
    fn formatting_a_text_range_normalises_with_whole_body_offsets() {
        let (mut s, doc_id, slide, shapes) = deck();
        s.apply_remote("text.set", &json!({"id": shapes[0].0, "text": "Hello world"})).expect("text");
        s.doc_mut().expect("doc").selection.shapes = vec![shapes[0]];
        caret(&mut s, shapes[0], (0, 5), None);
        s.doc_mut().expect("doc").selection.text.as_mut().expect("caret").anchor = (0, 0);
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());

        let ops = normalize(&km, st, "format.bold", &json!({"on": true}), "op", &doc_id).expect("a range in a shape");
        assert_eq!(
            ops[0].payload,
            Payload::FormatSet {
                format: FormatCmd::Bold,
                args: json!({"on": true}),
                addresses: vec![FormatAddress { cell: None, range: Some(TextRange { from: 0, to: 5 }), caret: None }]
            }
        );
        assert_eq!(ops[0].target.shape.as_ref().and_then(|k| km.shape_id(slide, k)), Some(shapes[0]));
        assert_eq!(ops[0].kind, OpKind::Command);

        // A range across a paragraph break counts the break as one character, the same
        // coordinates `text.delete` uses.
        s.apply_remote("text.set", &json!({"id": shapes[0].0, "text": "ab\ncd"})).expect("two paragraphs");
        caret(&mut s, shapes[0], (1, 1), None);
        s.doc_mut().expect("doc").selection.text.as_mut().expect("caret").anchor = (0, 1);
        let st = s.doc().expect("doc");
        let ops = normalize(&km, st, "format.italic", &json!({"on": true}), "op", &doc_id).expect("across paragraphs");
        assert!(
            matches!(&ops[0].payload, Payload::FormatSet { addresses, .. } if addresses == &[FormatAddress { cell: None, range: Some(TextRange { from: 1, to: 4 }), caret: None }]),
            "{:?}",
            ops[0].payload
        );

        // A caret in a shape's own body is its own address, not a word and not a refusal: the
        // engine command decides which characters it touches.
        s.doc_mut().expect("doc").selection.text.as_mut().expect("caret").anchor = (1, 1);
        let st = s.doc().expect("doc");
        let ops = normalize(&km, st, "format.bold", &json!({"on": true}), "op", &doc_id).expect("a caret in a shape");
        assert_eq!(
            ops[0].payload,
            Payload::FormatSet {
                format: FormatCmd::Bold,
                args: json!({"on": true}),
                addresses: vec![FormatAddress { cell: None, range: None, caret: Some(4) }]
            }
        );

        // With no text selection at all the whole body is addressed, as before.
        s.doc_mut().expect("doc").selection.text = None;
        s.apply_remote("text.set", &json!({"id": shapes[1].0, "text": "second"})).expect("text on the other shape");
        s.doc_mut().expect("doc").selection.shapes = vec![shapes[1]];
        let st = s.doc().expect("doc");
        let ops = normalize(&km, st, "format.bold", &json!({"on": true}), "op", &doc_id).expect("the whole body");
        assert_eq!(
            ops[0].payload,
            Payload::FormatSet { format: FormatCmd::Bold, args: json!({"on": true}), addresses: vec![FormatAddress::default()] }
        );
    }

    #[test]
    fn a_table_row_or_column_edit_normalises_to_one_structural_step() {
        let (mut s, doc_id, _slide, table) = table_deck();
        caret(&mut s, table, (0, 0), Some((0, 0)));
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());

        let ops = normalize(&km, st, "table.insertRowAbove", &json!({}), "op", &doc_id).expect("insert above");
        assert_eq!(ops[0].kind, OpKind::Structural);
        assert_eq!(ops[0].payload, Payload::TableEdit { axis: TableAxis::Row, at: 0, insert: true });

        let ops = normalize(&km, st, "table.insertRowBelow", &json!({}), "op", &doc_id).expect("insert below");
        assert_eq!(ops[0].payload, Payload::TableEdit { axis: TableAxis::Row, at: 1, insert: true });

        let ops = normalize(&km, st, "table.deleteRow", &json!({"row": 1}), "op", &doc_id).expect("delete row");
        assert_eq!(ops[0].payload, Payload::TableEdit { axis: TableAxis::Row, at: 1, insert: false });

        let ops = normalize(&km, st, "table.insertColumnRight", &json!({}), "op", &doc_id).expect("insert right");
        assert_eq!(ops[0].payload, Payload::TableEdit { axis: TableAxis::Column, at: 1, insert: true });

        let ops = normalize(&km, st, "table.deleteColumn", &json!({"col": 1}), "op", &doc_id).expect("delete column");
        assert_eq!(ops[0].payload, Payload::TableEdit { axis: TableAxis::Column, at: 1, insert: false });

        // An address outside the table is refused before anything is sent.
        assert!(normalize(&km, st, "table.deleteRow", &json!({"row": 9}), "op", &doc_id).is_err());
        assert!(normalize(&km, st, "table.deleteColumn", &json!({"col": 9}), "op", &doc_id).is_err());
    }

    #[test]
    fn the_rest_of_the_table_surface_normalises_to_final_values() {
        let (mut s, doc_id, _slide, table) = table_deck();
        caret(&mut s, table, (0, 0), Some((0, 0)));
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());

        let ops = normalize(&km, st, "table.style", &json!({"style": "grid"}), "op", &doc_id).expect("style");
        assert_eq!(ops[0].kind, OpKind::Command);
        assert_eq!(ops[0].payload, Payload::TableStyle { style: "grid".into() });
        assert!(normalize(&km, st, "table.style", &json!({"style": "  "}), "op", &doc_id).is_err());

        // A cell fill is always an explicit cell list: the caret's cell, then every cell of a
        // selected range, then every cell of the table.
        let ops = normalize(&km, st, "table.cellFill", &json!({"color": "#ff0000"}), "op", &doc_id).expect("the caret's cell");
        assert_eq!(ops[0].payload, Payload::TableCellFill { fill: CellFillSpec::Solid { color: "#ff0000".into() }, cells: vec![(0, 0)] });
        s.doc_mut().expect("doc").selection.cells = Some((table, (0, 0), (1, 1)));
        let st = s.doc().expect("doc");
        let ops = normalize(&km, st, "table.cellFill", &json!({"none": true}), "op", &doc_id).expect("the range");
        assert_eq!(ops[0].payload, Payload::TableCellFill { fill: CellFillSpec::None, cells: vec![(0, 0), (0, 1), (1, 0), (1, 1)] });
        s.doc_mut().expect("doc").selection.cells = None;
        s.doc_mut().expect("doc").selection.text = None;
        let st = s.doc().expect("doc");
        let ops = normalize(&km, st, "table.cellFill", &json!({}), "op", &doc_id).expect("every cell");
        assert!(matches!(&ops[0].payload, Payload::TableCellFill { fill: CellFillSpec::Inherit, cells } if cells.len() == 4), "{:?}", ops[0].payload);
        let ops = normalize(&km, st, "table.cellFill", &json!({"cells": [[1, 1]]}), "op", &doc_id).expect("an explicit cell");
        assert_eq!(ops[0].payload, Payload::TableCellFill { fill: CellFillSpec::Inherit, cells: vec![(1, 1)] });
        assert!(normalize(&km, st, "table.cellFill", &json!({"cells": [[9, 9]]}), "op", &doc_id).is_err());
        assert!(normalize(&km, st, "table.cellFill", &json!({"cells": []}), "op", &doc_id).is_err());

        // A merge takes its corners from the selection, and both corners must be cells of the table.
        s.doc_mut().expect("doc").selection.cells = Some((table, (0, 0), (1, 1)));
        let st = s.doc().expect("doc");
        let ops = normalize(&km, st, "table.merge", &json!({}), "op", &doc_id).expect("merge the selection");
        assert_eq!(ops[0].payload, Payload::TableMerge { from: (0, 0), to: (1, 1) });
        assert!(normalize(&km, st, "table.merge", &json!({"from": [0, 0], "to": [0, 0]}), "op", &doc_id).is_err());
        assert!(normalize(&km, st, "table.merge", &json!({"from": [0, 0], "to": [2, 2]}), "op", &doc_id).is_err());

        // A split addresses a merged cell: once the table has one, the caret can name it.
        s.apply_remote("table.merge", &json!({"id": table.0, "from": [0, 0], "to": [1, 1]})).expect("merge");
        let st = s.doc().expect("doc");
        let ops = normalize(&km, st, "table.split", &json!({"cell": [0, 0]}), "op", &doc_id).expect("split");
        assert_eq!(ops[0].payload, Payload::TableSplit { cell: (0, 0) });
        assert!(normalize(&km, st, "table.split", &json!({"cell": [1, 1]}), "op", &doc_id).is_err(), "a covered cell is not a merge anchor");

        // Sizing: final numbers, clamped as the single-user command clamps them.
        let ops = normalize(&km, st, "table.columnWidth", &json!({"col": 0, "width": 120, "keepTotal": true}), "op", &doc_id).expect("width");
        assert_eq!(ops[0].payload, Payload::TableColumnWidth { col: 0, width: 120.0, keep_total: true });
        let ops = normalize(&km, st, "table.columnWidth", &json!({"col": 1, "width": 1}), "op", &doc_id).expect("clamped width");
        assert_eq!(ops[0].payload, Payload::TableColumnWidth { col: 1, width: 4.0, keep_total: false });
        let ops = normalize(&km, st, "table.rowHeight", &json!({"row": 1, "height": 40}), "op", &doc_id).expect("height");
        assert_eq!(ops[0].payload, Payload::TableRowHeight { row: 1, height: 40.0 });
        assert!(normalize(&km, st, "table.columnWidth", &json!({"col": 9, "width": 100}), "op", &doc_id).is_err());
        assert!(normalize(&km, st, "table.rowHeight", &json!({"row": 9, "height": 40}), "op", &doc_id).is_err());
        assert!(normalize(&km, st, "table.columnWidth", &json!({"col": 0}), "op", &doc_id).is_err());

        // A shape that is not a table is refused for every one of them.
        let rect =
            s.execute("shape.insert", &json!({"preset": "rect", "rect": [500, 100, 100, 100]})).expect("rect")["id"].as_u64().expect("id") as u32;
        let st = s.doc().expect("doc");
        for (cmd, params) in [
            ("table.style", json!({"id": rect, "style": "grid"})),
            ("table.options", json!({"id": rect, "headerRow": true})),
            ("table.cellFill", json!({"id": rect})),
            ("table.merge", json!({"id": rect, "from": [0, 0], "to": [0, 1]})),
            ("table.split", json!({"id": rect})),
            ("table.columnWidth", json!({"id": rect, "width": 90})),
            ("table.rowHeight", json!({"id": rect, "height": 40})),
        ] {
            assert!(normalize(&km, st, cmd, &params, "op", &doc_id).is_err(), "{cmd} on a rectangle");
        }
    }

    #[test]
    fn table_options_normalise_to_one_final_set() {
        let (mut s, doc_id, _slide, table) = table_deck();
        caret(&mut s, table, (0, 0), Some((0, 0)));
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());

        // The style options are six independent booleans, so the operation carries the whole set:
        // the sender applies the flags it names to the table it sees and sends every final value.
        let ops = normalize(&km, st, "table.options", &json!({"headerRow": false, "lastColumn": true}), "op", &doc_id).expect("options");
        assert_eq!(ops[0].kind, OpKind::Command);
        assert_eq!(
            ops[0].payload,
            Payload::TableOptions {
                header_row: false,
                total_row: false,
                banded_rows: true,
                first_column: false,
                last_column: true,
                banded_columns: false,
            }
        );

        // A misspelt flag, a value that is not a boolean and a call that names no option are
        // refused before anything is sent.
        assert!(normalize(&km, st, "table.options", &json!({"headerRow": false, "bandedRow": true}), "op", &doc_id).is_err());
        assert!(normalize(&km, st, "table.options", &json!({"headerRow": 1}), "op", &doc_id).is_err());
        assert!(normalize(&km, st, "table.options", &json!({"headerRow": null}), "op", &doc_id).is_err());
        assert!(normalize(&km, st, "table.options", &json!({}), "op", &doc_id).is_err());
    }

    /// One slide with a SmartArt graphic (three nodes: Plan, then Scope under it, then Build).
    fn smartart_deck() -> (Session, String, SlideId, ShapeId) {
        let mut s = Session::with_new();
        let title = s.doc().expect("doc").doc.slides[0].id;
        s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("blank");
        s.apply_remote("slide.delete", &json!({"ids": [title.0]})).expect("drop title");
        let r = s.apply_remote("insert.smartArt", &json!({"kind": "blockList", "items": ["Plan", "\tScope", "Build"]})).expect("smartart");
        let gid = ShapeId(r["id"].as_u64().expect("id") as u32);
        let slide = s.doc().expect("doc").doc.slides[0].id;
        (s, "d".into(), slide, gid)
    }

    #[test]
    fn smartart_commands_normalise_to_one_final_diagram() {
        let (s, doc, slide, group) = smartart_deck();
        let child = match &s.doc().expect("doc").doc.slides[0].shapes[0].kind {
            ShapeKind::Group { children, .. } => children[1].id,
            other => panic!("not a group: {other:?}"),
        };
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());

        // An outline edit sends the whole resulting value: the layout and every node, with the
        // typed sub-points' levels — not the lines the user typed.
        let ops = normalize(&km, st, "smartArt.setNodes", &json!({"lines": ["Plan", "\tScope", "\tRisks", "Build"]}), "op", &doc).expect("set nodes");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].kind, OpKind::Command);
        assert_eq!(ops[0].target.slide.as_ref().and_then(|k| km.slide_id(k)), Some(slide));
        assert_eq!(ops[0].target.shape.as_ref().and_then(|k| km.shape_id(slide, k)), Some(group));
        let Payload::SmartArtSet { diagram } = &ops[0].payload else { panic!("{:?}", ops[0].payload) };
        assert_eq!(diagram["kind"], "blockList");
        let nodes: Vec<(&str, u64)> =
            diagram["nodes"].as_array().expect("nodes").iter().map(|n| (n["text"].as_str().unwrap(), n["level"].as_u64().unwrap())).collect();
        assert_eq!(nodes, [("Plan", 0), ("Scope", 1), ("Risks", 1), ("Build", 0)]);

        // A child shape addresses its group, like the single-user command; an outline step keeps
        // the layout and changes one node.
        let ops = normalize(&km, st, "smartArt.promote", &json!({"id": child.0, "index": 1}), "op", &doc).expect("promote");
        let Payload::SmartArtSet { diagram } = &ops[0].payload else { panic!("{:?}", ops[0].payload) };
        assert_eq!(diagram["kind"], "blockList");
        assert_eq!(diagram["nodes"][1]["level"], 0, "Scope is promoted");
        assert_eq!(ops[0].target.shape.as_ref().and_then(|k| km.shape_id(slide, k)), Some(group));

        // Layout, style and colours are the same whole value: the outline is kept.
        for (cmd, p, check) in [
            ("smartArt.layout", json!({"kind": "hierarchy"}), "kind"),
            ("smartArt.style", json!({"style": "intense"}), "style"),
            ("smartArt.colors", json!({"colors": "accent3"}), "colors"),
        ] {
            let ops = normalize(&km, st, cmd, &p, "op", &doc).expect(cmd);
            let Payload::SmartArtSet { diagram } = &ops[0].payload else { panic!("{cmd}: {:?}", ops[0].payload) };
            let value = p[check].as_str().unwrap();
            assert_eq!(diagram[check], value, "{cmd}");
            assert_eq!(diagram["nodes"].as_array().expect("nodes").len(), 3, "{cmd} keeps the outline");
        }

        // Add and remove send the resulting list too.
        let ops = normalize(&km, st, "smartArt.addNode", &json!({"index": 2, "position": "after", "text": "Ship"}), "op", &doc).expect("add");
        let Payload::SmartArtSet { diagram } = &ops[0].payload else { panic!("{:?}", ops[0].payload) };
        assert_eq!(diagram["nodes"].as_array().expect("nodes").len(), 4);
        let ops = normalize(&km, st, "smartArt.removeNode", &json!({"index": 0}), "op", &doc).expect("remove");
        let Payload::SmartArtSet { diagram } = &ops[0].payload else { panic!("{:?}", ops[0].payload) };
        let texts: Vec<&str> = diagram["nodes"].as_array().expect("nodes").iter().map(|n| n["text"].as_str().unwrap()).collect();
        assert_eq!(texts, ["Scope", "Build"]);
    }

    #[test]
    fn smartart_refusals_happen_before_anything_is_sent() {
        let (mut s, doc, _slide, group) = smartart_deck();
        let rect = s.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 50, 50]})).expect("rect")["id"].as_u64().unwrap();
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());
        // The selection is a plain rectangle: no graphic to address.
        assert!(matches!(normalize(&km, st, "smartArt.layout", &json!({"kind": "hierarchy"}), "op", &doc), Err(LocalError::BadParams(_))));
        assert!(matches!(
            normalize(&km, st, "smartArt.layout", &json!({"id": rect, "kind": "hierarchy"}), "op", &doc),
            Err(LocalError::BadParams(_))
        ));
        // An unknown layout, style or colour, and a node that does not exist, are refused here.
        assert!(matches!(normalize(&km, st, "smartArt.layout", &json!({"id": group.0, "kind": "nope"}), "op", &doc), Err(LocalError::BadParams(_))));
        assert!(matches!(normalize(&km, st, "smartArt.style", &json!({"id": group.0, "style": "nope"}), "op", &doc), Err(LocalError::BadParams(_))));
        assert!(matches!(
            normalize(&km, st, "smartArt.colors", &json!({"id": group.0, "colors": "accent9"}), "op", &doc),
            Err(LocalError::BadParams(_))
        ));
        assert!(matches!(normalize(&km, st, "smartArt.removeNode", &json!({"id": group.0, "index": 99}), "op", &doc), Err(LocalError::BadParams(_))));
        assert!(matches!(normalize(&km, st, "smartArt.promote", &json!({"id": group.0, "index": 0}), "op", &doc), Err(LocalError::BadParams(_))));
        assert!(matches!(
            normalize(&km, st, "smartArt.setNodes", &json!({"id": group.0, "index": 99, "text": "x"}), "op", &doc),
            Err(LocalError::BadParams(_))
        ));
        // The three conversions are structural now: each normalises to its own final value.
        assert!(normalize(&km, st, "smartArt.toText", &json!({"id": group.0}), "op", &doc).is_ok());
    }

    #[test]
    fn smartart_conversions_normalise_to_their_whole_result_as_structural_steps() {
        let (mut s, doc, slide, group) = smartart_deck();
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());

        // To Text: one whole text body with the outline's levels and the graphic's own font
        // sizes, and the graphic's final box.
        let ops = normalize(&km, st, "smartArt.toText", &json!({"id": group.0}), "op", &doc).expect("toText");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].kind, OpKind::Structural);
        assert_eq!(ops[0].target.shape.as_ref().and_then(|k| km.shape_id(slide, k)), Some(group));
        let Payload::SmartArtToText { body, rect } = &ops[0].payload else { panic!("{:?}", ops[0].payload) };
        let paragraphs = body["paragraphs"].as_array().expect("paragraphs");
        let texts: Vec<&str> =
            paragraphs.iter().map(|p| p["runs"].as_array().and_then(|r| r.first()).and_then(|r| r["text"].as_str()).unwrap_or_default()).collect();
        let levels: Vec<u64> = paragraphs.iter().map(|p| p["level"].as_u64().unwrap_or(0)).collect();
        assert_eq!(texts, ["Plan", "Scope", "Build"]);
        assert_eq!(levels, [0, 1, 0], "the outline levels travel with the text");
        assert!(rect[2] > 0.0 && rect[3] > 0.0, "the graphic's box is kept: {rect:?}");

        // To Shapes: the drawing stays, only the outline is dropped, so no value travels.
        let ops = normalize(&km, st, "smartArt.toShapes", &json!({"id": group.0}), "op", &doc).expect("toShapes");
        assert_eq!(ops[0].kind, OpKind::Structural);
        assert_eq!(ops[0].payload, Payload::SmartArtToShapes {});
        assert_eq!(ops[0].target.shape.as_ref().and_then(|k| km.shape_id(slide, k)), Some(group));

        // From Text: the selected text box's paragraphs become the graphic's nodes, and the new
        // graphic gets a box a diagram can use.
        let text = s.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [10, 20, 120, 60], "text": "Alpha\nBeta"})).expect("text");
        let text_id = ShapeId(text["id"].as_u64().expect("id") as u32);
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());
        let ops = normalize(&km, st, "smartArt.fromText", &json!({"id": text_id.0, "kind": "process"}), "op", &doc).expect("fromText");
        assert_eq!(ops[0].kind, OpKind::Structural);
        assert_eq!(ops[0].target.shape.as_ref().and_then(|k| km.shape_id(slide, k)), Some(text_id));
        let Payload::SmartArtFromText { diagram, rect } = &ops[0].payload else { panic!("{:?}", ops[0].payload) };
        assert_eq!(diagram["kind"], "process");
        let texts: Vec<&str> = diagram["nodes"].as_array().expect("nodes").iter().map(|n| n["text"].as_str().unwrap()).collect();
        assert_eq!(texts, ["Alpha", "Beta"]);
        assert!(rect[2] >= st.doc.slide_size.width * 0.5, "a small text box grows for the diagram: {rect:?}");

        // The three refusals happen here, before anything is queued: no text to convert, no
        // graphic to convert, and a command outside the conversion surface.
        assert!(matches!(normalize(&km, st, "smartArt.fromText", &json!({"id": group.0}), "op", &doc), Err(LocalError::BadParams(_))));
        assert!(matches!(normalize(&km, st, "smartArt.toText", &json!({"id": text_id.0}), "op", &doc), Err(LocalError::BadParams(_))));
        assert!(matches!(normalize(&km, st, "smartArt.toShapes", &json!({"id": text_id.0}), "op", &doc), Err(LocalError::BadParams(_))));
    }

    #[test]
    fn a_node_picture_normalises_to_the_media_reference_and_its_place() {
        let (mut s, doc, slide, group) = smartart_deck();
        let rect = s.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 80, 80]})).expect("rect")["id"].as_u64().unwrap();
        let child = match &s.doc().expect("doc").doc.slides[0].shapes[0].kind {
            ShapeKind::Group { children, .. } => children[1].id,
            other => panic!("not a group: {other:?}"),
        };
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());
        let sha = "a".repeat(64);
        let ops = normalize(
            &km,
            st,
            "smartArt.picture",
            &json!({"id": child.0, "index": 1, "sha256": sha, "name": "p.png", "place": {"crop": [0.1, 0.0, 0.2, 0.0], "alpha": 0.8}}),
            "op",
            &doc,
        )
        .expect("picture");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].kind, OpKind::Command, "one final value, not a structural step");
        assert_eq!(ops[0].target.slide.as_ref().and_then(|k| km.slide_id(k)), Some(slide));
        assert_eq!(ops[0].target.shape.as_ref().and_then(|k| km.shape_id(slide, k)), Some(group), "a child addresses its graphic");
        let Payload::SmartArtPicture { node, media } = &ops[0].payload else { panic!("{:?}", ops[0].payload) };
        assert_eq!(*node, 0, "a sub-point's picture is its top-level node's");
        let media = media.as_ref().expect("the media reference travels");
        assert_eq!(media.sha256, sha);
        assert_eq!(media.name, "p.png");
        assert_eq!(media.place.crop, [0.1, 0.0, 0.2, 0.0]);
        assert_eq!(media.place.alpha, Some(0.8));

        // Reset names no media at all.
        let ops = normalize(&km, st, "smartArt.picture", &json!({"id": group.0, "index": 0, "reset": true}), "op", &doc).expect("reset");
        assert_eq!(ops[0].payload, Payload::SmartArtPicture { node: 0, media: None });

        // A plain shape, a node the outline does not have, an unknown parameter and a place that
        // leaves no area are refused here, before anything is queued.
        assert!(matches!(
            normalize(&km, st, "smartArt.picture", &json!({"id": rect, "index": 0, "sha256": sha}), "op", &doc),
            Err(LocalError::BadParams(_))
        ));
        for bad in [
            json!({"id": group.0, "index": 9, "sha256": sha}),
            json!({"id": group.0, "index": 0, "sha256": sha, "face": "regular"}),
            json!({"id": group.0, "index": 0, "sha256": sha, "place": {"crop": [0.9, 0.0, 0.9, 0.0]}}),
            json!({"id": group.0, "index": 0, "sha256": "abc"}),
        ] {
            assert!(normalize(&km, st, "smartArt.picture", &bad, "op", &doc).is_err(), "{bad}");
        }
    }

    /// One slide with a chart (a column chart with explicit categories and one series).
    fn chart_deck() -> (Session, String, SlideId, ShapeId) {
        let mut s = Session::with_new();
        let title = s.doc().expect("doc").doc.slides[0].id;
        s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("blank");
        s.apply_remote("slide.delete", &json!({"ids": [title.0]})).expect("drop title");
        let r = s
            .apply_remote("insert.chart", &json!({"type": "column", "categories": ["Q1", "Q2"], "series": [{"name": "Sales", "values": [4.0, 5.0]}]}))
            .expect("chart");
        let id = ShapeId(r["id"].as_u64().expect("id") as u32);
        let slide = s.doc().expect("doc").doc.slides[0].id;
        (s, "d".into(), slide, id)
    }

    #[test]
    fn chart_commands_normalise_to_one_whole_chart_value() {
        let (s, doc, slide, chart) = chart_deck();
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());

        // A type change sends the whole chart: the new type and the data it now draws.
        let ops = normalize(&km, st, "chart.type", &json!({"type": "pie"}), "op", &doc).expect("type");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].kind, OpKind::Command);
        assert_eq!(ops[0].target.slide.as_ref().and_then(|k| km.slide_id(k)), Some(slide));
        assert_eq!(ops[0].target.shape.as_ref().and_then(|k| km.shape_id(slide, k)), Some(chart));
        let Payload::ChartSet { chart: value } = &ops[0].payload else { panic!("{:?}", ops[0].payload) };
        assert_eq!(value["kind"], "pie");
        assert_eq!(value["categories"], json!(["Q1", "Q2"]), "the data travels with the type");
        assert_eq!(value["series"][0]["values"], json!([4.0, 5.0]));

        // A data change keeps the type; options travel with the type and the data.
        let ops =
            normalize(&km, st, "chart.data", &json!({"categories": ["A", "B", "C"], "series": [{"name": "S", "values": [1, 2, 3]}]}), "op", &doc)
                .expect("data");
        let Payload::ChartSet { chart: value } = &ops[0].payload else { panic!("{:?}", ops[0].payload) };
        assert_eq!(value["kind"], "column");
        assert_eq!(value["categories"], json!(["A", "B", "C"]));
        assert_eq!(value["series"][0]["name"], "S");

        let ops = normalize(&km, st, "chart.options", &json!({"title": "Revenue", "legend": "b"}), "op", &doc).expect("options");
        let Payload::ChartSet { chart: value } = &ops[0].payload else { panic!("{:?}", ops[0].payload) };
        assert_eq!(value["title"], "Revenue");
        assert_eq!(value["legend"], "b");
        assert_eq!(value["categories"], json!(["Q1", "Q2"]), "the data travels with the options");
    }

    #[test]
    fn a_chart_insertion_normalises_to_the_whole_chart_it_would_create() {
        let (s, doc, _slide, _chart) = chart_deck();
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());
        let ops = normalize(
            &km,
            st,
            "insert.chart",
            &json!({"type": "bar", "categories": ["A"], "series": [{"name": "S", "values": [2.5]}], "rect": [10, 20, 300, 200]}),
            "op",
            &doc,
        )
        .expect("insert");
        assert_eq!(ops[0].kind, OpKind::Structural);
        let Payload::ChartInsert { chart, rect } = &ops[0].payload else { panic!("{:?}", ops[0].payload) };
        assert_eq!(chart["kind"], "bar");
        assert_eq!(chart["series"][0]["values"], json!([2.5]));
        assert_eq!(*rect, Some([10.0, 20.0, 300.0, 200.0]));
        // Without a box the operation still carries the author's chart; the box is the command's
        // default on every replica.
        let ops = normalize(&km, st, "insert.chart", &json!({"type": "line"}), "op", &doc).expect("default box");
        assert!(matches!(ops[0].payload, Payload::ChartInsert { rect: None, .. }));
        // A parameter the chart plan does not carry is refused rather than dropped.
        assert!(matches!(normalize(&km, st, "insert.chart", &json!({"type": "line", "3d": true}), "op", &doc), Err(LocalError::Unsupported(_))));
        assert!(normalize(&km, st, "insert.chart", &json!({"rect": [1, 2]}), "op", &doc).is_err());
    }

    #[test]
    fn a_smartart_insertion_normalises_to_the_whole_diagram_it_would_create() {
        let (s, doc, slide, _group) = smartart_deck();
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());

        // The layout and the whole node tree travel as one final value; the sender built them
        // with the same plan the single-user command runs.
        let params = json!({"kind": "hierarchy", "items": ["CEO", "\tCTO", "\t\tDev"], "rect": [40, 50, 400, 300]});
        let ops = normalize(&km, st, "insert.smartArt", &params, "op", &doc).expect("insert");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].kind, OpKind::Structural);
        assert_eq!(ops[0].target.slide.as_ref().and_then(|k| km.slide_id(k)), Some(slide));
        let Payload::SmartArtInsert { diagram, rect } = &ops[0].payload else { panic!("{:?}", ops[0].payload) };
        assert_eq!(diagram["kind"], "hierarchy");
        let nodes: Vec<(&str, u64)> =
            diagram["nodes"].as_array().expect("nodes").iter().map(|n| (n["text"].as_str().unwrap(), n["level"].as_u64().unwrap())).collect();
        assert_eq!(nodes, [("CEO", 0), ("CTO", 1), ("Dev", 2)]);
        assert_eq!(*rect, Some([40.0, 50.0, 400.0, 300.0]));
        let planned = deckcraft_engine::cmd::smartart::insert_plan(&params).expect("plan");
        assert_eq!(serde_json::to_value(&planned).expect("value"), *diagram, "the normaliser sends what the command would build");

        // Without parameters the command's defaults travel too: a `process` graphic with three
        // `Text` nodes and the command's own box.
        let ops = normalize(&km, st, "insert.smartArt", &json!({"kind": "nope"}), "op", &doc).expect("defaults");
        let Payload::SmartArtInsert { diagram, rect } = &ops[0].payload else { panic!("{:?}", ops[0].payload) };
        assert_eq!(diagram["kind"], "process");
        assert_eq!(diagram["nodes"].as_array().expect("nodes").len(), 3);
        assert_eq!(diagram["nodes"][0]["text"], "Text");
        assert!(rect.is_none());

        // A parameter the plan does not carry, an unreadable node list and a box that is not four
        // numbers are refused rather than silently dropped.
        assert!(matches!(normalize(&km, st, "insert.smartArt", &json!({"kind": "list", "3d": true}), "op", &doc), Err(LocalError::Unsupported(_))));
        assert!(matches!(normalize(&km, st, "insert.smartArt", &json!({"nodes": [{"text": 1}]}), "op", &doc), Err(LocalError::BadParams(_))));
        assert!(normalize(&km, st, "insert.smartArt", &json!({"rect": [1, 2]}), "op", &doc).is_err());
    }

    #[test]
    fn chart_refusals_happen_before_anything_is_sent() {
        let (mut s, doc, _slide, chart) = chart_deck();
        let rect = s.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [400, 100, 50, 50]})).expect("rect")["id"].as_u64().unwrap();
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());
        // A shape that is not a chart is refused for every command of the slice.
        for cmd in ["chart.type", "chart.data", "chart.options"] {
            assert!(matches!(normalize(&km, st, cmd, &json!({"id": rect}), "op", &doc), Err(LocalError::BadParams(_))), "{cmd}");
        }
        // A bad bin width and a bad text colour are refused here, before anything is queued.
        assert!(matches!(normalize(&km, st, "chart.options", &json!({"id": chart.0, "binSize": -1}), "op", &doc), Err(LocalError::BadParams(_))));
        assert!(matches!(
            normalize(&km, st, "chart.options", &json!({"id": chart.0, "textColor": "reddish"}), "op", &doc),
            Err(LocalError::BadParams(_))
        ));
    }

    #[test]
    fn notes_commands_normalise_to_one_slide_body_step() {
        let (mut s, doc_id, slide, _) = deck();
        s.apply_remote("slide.notes", &json!({"index": 0, "text": "old"})).expect("seed");
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());
        let body = st.doc.slides[0].notes.clone();
        let whole = serde_json::to_value(&body).expect("body json");

        // The notes pane sends the formatted body: the operation carries the whole body.
        let ops = normalize(&km, st, "slide.notes", &json!({"body": whole.clone(), "index": 0}), "op", &doc_id).expect("body");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].kind, OpKind::Command);
        assert_eq!(ops[0].target.slide.as_ref().and_then(|k| km.slide_id(k)), Some(slide));
        assert!(ops[0].target.shape.is_none(), "notes belong to the slide, not to a shape");
        assert_eq!(ops[0].payload, Payload::NotesSet { notes: whole.clone() });

        // Typed text becomes the same whole body the single-user command would leave behind.
        let ops = normalize(&km, st, "slide.notes", &json!({"text": "new"}), "op", &doc_id).expect("text");
        let Payload::NotesSet { notes } = &ops[0].payload else { panic!("{:?}", ops[0].payload) };
        let sent = serde_json::from_value::<deckcraft_model::text::TextBody>(notes.clone()).expect("a text body");
        assert_eq!(sent.text(), "new");

        // A slide that does not exist is refused before anything is sent.
        assert!(matches!(normalize(&km, st, "slide.notes", &json!({"text": "x", "index": 9}), "op", &doc_id), Err(LocalError::BadParams(_))));
        assert!(matches!(normalize(&km, st, "slide.notes", &json!({}), "op", &doc_id), Err(LocalError::BadParams(_))));

        // A caret in the notes turns typing and deleting into the slide body's own offsets.
        let mut caret = TextSel { shape: ShapeId(0), anchor: (0, 3), caret: (0, 3), cell: None, notes: true, line_end: None };
        s.doc_mut().expect("doc").selection.text = Some(caret.clone());
        let st = s.doc().expect("doc");
        let ops = normalize(&km, st, "text.insert", &json!({"text": "!"}), "op", &doc_id).expect("insert");
        assert_eq!(ops[0].target.slide.as_ref().and_then(|k| km.slide_id(k)), Some(slide));
        assert!(ops[0].target.shape.is_none());
        assert_eq!(ops[0].payload, Payload::NotesInsert { at: 3, text: "!".into() });

        caret.anchor = (0, 0);
        s.doc_mut().expect("doc").selection.text = Some(caret);
        let st = s.doc().expect("doc");
        let ops = normalize(&km, st, "text.delete", &json!({}), "op", &doc_id).expect("delete");
        assert_eq!(ops[0].payload, Payload::NotesDelete { from: 0, to: 3 });
        let ops = normalize(&km, st, "edit.delete", &json!({}), "op", &doc_id).expect("edit.delete");
        assert_eq!(ops[0].payload, Payload::NotesDelete { from: 0, to: 3 });
    }

    /// A small original font the model carries (ColrSquares), as the UI would pass it.
    fn font_file() -> Vec<u8> {
        include_bytes!("../../render/tests/fixtures/colr-squares.ttf").to_vec()
    }

    #[test]
    fn a_font_command_uploads_bytes_and_sends_only_a_reference() {
        let (mut s, _doc, _slide, _shapes) = deck();
        let mut client = CollabDoc::new("d");
        client.connect(&s, "jwt", ClientInfo { name: "test".into(), ver: "1".into() });
        client.on_msg(&mut s, welcome(0));
        let bytes = font_file();
        let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let sent =
            client.submit(&mut s, "font.create", &json!({"typeface": "Deck Test Face", "face": "bold", "data": data}), "op-f").expect("submit");
        assert_eq!(sent, 1);
        let Some(ClientMsg::Op(op)) = client.next_outgoing() else { panic!("expected an operation") };
        match &op.payload {
            Payload::FontCreate { typeface, face, sha256, name } => {
                assert_eq!(typeface, "Deck Test Face");
                assert_eq!(*face, crate::FontFace::Bold);
                assert_eq!(sha256, &crate::media::sha256_hex(&bytes), "the operation names the bytes by their digest");
                assert_eq!(name, "Deck Test Face.ttf", "a font names its object after the typeface unless told otherwise");
                assert!(!serde_json::to_string(&op.payload).expect("json").contains("data"), "no bytes on the wire");
            }
            other => panic!("{other:?}"),
        }
        assert!(op.target.slide.is_none() && op.target.shape.is_none(), "a document font has no slide address");
        assert_eq!(op.kind, OpKind::Command);
        assert_eq!(client.take_uploads().len(), 1, "the bytes go to the media channel first");
        let doc = s.doc().expect("doc").doc.clone();
        assert!(doc.embedded_fonts.is_empty(), "a submit applies nothing locally");
    }

    #[test]
    fn font_commands_normalise_to_final_values_and_refuse_bad_arguments() {
        let (s, doc_id, _slide, _shapes) = deck();
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());
        let sha = crate::media::sha256_hex(&font_file());

        let ops = normalize(&km, st, "font.create", &json!({"typeface": "Deck Test Face", "sha256": sha, "name": "face.ttf"}), "op", &doc_id)
            .expect("create");
        assert_eq!(
            ops[0].payload,
            Payload::FontCreate { typeface: "Deck Test Face".into(), face: crate::FontFace::Regular, sha256: sha.clone(), name: "face.ttf".into() }
        );
        assert_eq!(ops[0].kind, OpKind::Command);
        assert!(ops[0].target.slide.is_none());

        let ops =
            normalize(&km, st, "font.set", &json!({"typeface": "Deck Test Face", "panose": "020B0604020202020204"}), "op", &doc_id).expect("set");
        assert_eq!(
            ops[0].payload,
            Payload::FontSet { typeface: "Deck Test Face".into(), panose: Some("020B0604020202020204".into()), pitch_family: None, charset: None }
        );

        let ops = normalize(&km, st, "font.remove", &json!({"typeface": "Deck Test Face"}), "op", &doc_id).expect("remove");
        assert_eq!(ops[0].payload, Payload::FontRemove { typeface: "Deck Test Face".into() });

        for (why, cmd, params) in [
            ("a missing typeface", "font.create", json!({"sha256": sha, "name": "x"})),
            ("an empty typeface", "font.remove", json!({"typeface": "  "})),
            ("an unknown face", "font.create", json!({"typeface": "X", "face": "black", "sha256": sha, "name": "x"})),
            ("a digest that is not a sha256", "font.create", json!({"typeface": "X", "sha256": "abc", "name": "x"})),
            ("a bad panose", "font.set", json!({"typeface": "X", "panose": "xyz"})),
            ("a charset out of range", "font.set", json!({"typeface": "X", "charset": 300})),
            ("nothing to set", "font.set", json!({"typeface": "X"})),
            ("an address in the arguments", "font.remove", json!({"typeface": "X", "id": 3})),
        ] {
            let e = normalize(&km, st, cmd, &params, "op", &doc_id).expect_err(why);
            assert!(matches!(e, LocalError::BadParams(_) | LocalError::Unsupported(_)), "{why}: {e:?}");
        }
    }

    #[test]
    fn commands_outside_the_v1_set_are_refused_by_name() {
        let (s, doc, _, _) = deck();
        let km = keys(s.doc().expect("doc").doc.as_ref());
        let e = normalize(&km, s.doc().expect("doc"), "file.saveAs", &json!({}), "op", &doc).expect_err("refused");
        assert_eq!(e, LocalError::Unsupported("file.saveAs".into()));
    }

    #[test]
    fn fill_line_and_format_normalise_explicit_shapes() {
        let (s, doc, slide, shapes) = deck();
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());
        let key = km.shape_key(slide, shapes[0]).expect("key").clone();

        let ops = normalize(&km, st, "shape.fill", &json!({"color": "#ff0000", "id": shapes[0].0}), "op", &doc).expect("fill");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].target.shape.as_ref(), Some(&key));
        assert_eq!(ops[0].payload, Payload::ShapeFill { fill: FillSpec::Solid { color: "#ff0000".into() } });
        assert_eq!(ops[0].kind, OpKind::Command);

        let ops =
            normalize(&km, st, "shape.fill", &json!({"gradient": {"stops": []}, "id": shapes[0].0}), "op", &doc).expect_err("relative/gradient fill");
        assert!(matches!(ops, LocalError::Unsupported(_)) || matches!(ops, LocalError::BadParams(_)));

        let ops = normalize(&km, st, "shape.line", &json!({"color": "#0044ff", "width": 2.0, "id": shapes[0].0}), "op", &doc).expect("line");
        match &ops[0].payload {
            Payload::ShapeLine { line } => {
                assert_eq!(line.color.as_deref(), Some("#0044ff"));
                assert_eq!(line.width, Some(2.0));
            }
            other => panic!("{other:?}"),
        }

        let ops = normalize(&km, st, "format.bold", &json!({"on": true, "id": shapes[0].0}), "op", &doc).expect("format");
        assert_eq!(
            ops[0].payload,
            Payload::FormatSet { format: FormatCmd::Bold, args: json!({"on": true}), addresses: vec![FormatAddress::default()] }
        );
        let e = normalize(&km, st, "format.bold", &json!({"id": shapes[0].0}), "op", &doc).expect_err("relative toggle");
        assert!(matches!(e, LocalError::BadParams(_)), "{e:?}");
        let second_key = km.shape_key(slide, shapes[1]).expect("key").clone();
        let ops = normalize(&km, st, "format.bold", &json!({"on": true, "ids": [shapes[1].0]}), "op", &doc)
            .expect("the address is taken from the command params, not from the args");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].target.shape.as_ref(), Some(&second_key));
        assert_eq!(
            ops[0].payload,
            Payload::FormatSet { format: FormatCmd::Bold, args: json!({"on": true}), addresses: vec![FormatAddress::default()] }
        );
    }

    #[test]
    fn slide_duplicate_targets_the_source_slide() {
        let (s, doc, slide, _) = deck();
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());
        let ops = normalize(&km, st, "slide.duplicate", &json!({"index": 0}), "op", &doc).expect("duplicate");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].payload, Payload::SlideDuplicate {});
        assert_eq!(ops[0].kind, OpKind::Structural);
        assert_eq!(ops[0].target.slide.as_ref(), km.slide_key(slide));
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
        let sent = client.submit(&mut s, "insert.picture", &json!({"name": "red.png", "data": data}), "op-1").expect("submit");
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
    fn a_node_picture_command_uploads_its_bytes_and_sends_only_a_reference() {
        let (mut s, _doc, _slide, group) = smartart_deck();
        let mut client = CollabDoc::new("d");
        client.connect(&s, "jwt", ClientInfo { name: "test".into(), ver: "1".into() });
        client.on_msg(&mut s, welcome(0));
        let bytes = png();
        let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let sent = client
            .submit(
                &mut s,
                "smartArt.picture",
                &json!({"id": group.0, "index": 0, "name": "red.png", "data": data, "place": {"crop": [0.1, 0.0, 0.2, 0.0]}}),
                "op-1",
            )
            .expect("submit");
        assert_eq!(sent, 1);
        let Some(ClientMsg::Op(op)) = client.next_outgoing() else { panic!("expected an operation") };
        let Payload::SmartArtPicture { node, media } = &op.payload else { panic!("{:?}", op.payload) };
        assert_eq!(*node, 0);
        let media = media.as_ref().expect("the reference travels");
        assert_eq!(media.name, "red.png");
        assert_eq!(media.sha256, crate::media::sha256_hex(&bytes));
        assert_eq!(media.place.crop, [0.1, 0.0, 0.2, 0.0]);
        assert!(!serde_json::to_string(&op.payload).expect("json").contains("data"), "no bytes on the wire");
        assert!(client.has_media(&crate::media::sha256_hex(&bytes)));
        assert_eq!(op.kind, OpKind::Command);

        // A viewer's submit is refused on the client, before anything is queued.
        let mut viewer = CollabDoc::new("d");
        viewer.connect(&s, "jwt-v", ClientInfo { name: "viewer".into(), ver: "1".into() });
        let mut w = welcome(0);
        if let ServerMsg::Welcome(ref mut welcome) = w {
            welcome.role = deckcraft_collab_proto::Role::Viewer;
        }
        viewer.on_msg(&mut s, w);
        let sha = crate::media::sha256_hex(&png());
        assert_eq!(viewer.submit(&mut s, "smartArt.picture", &json!({"id": group.0, "index": 0, "sha256": sha}), "v-1"), Err(LocalError::ReadOnly));
    }

    #[test]
    fn a_model_camera_normalises_to_the_whole_final_view_and_a_relative_edit_is_refused() {
        let (mut s, doc, _slide, shapes) = deck();
        let data = base64::engine::general_purpose::STANDARD.encode(glb());
        let id = s.apply_remote("insert.model3d", &json!({"name": "block.glb", "data": data})).expect("model")["id"].as_u64().unwrap();
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());

        // The whole final view travels; the address is the model, the command is a value command.
        let view =
            json!({"id": id, "camera": {"position": [0.0, 0.0, 11713743.0], "target": [0.0, 0.0, 0.0], "up": [0.0, 36000000.0, 0.0], "fov": 45.0}});
        let ops = normalize(&km, st, "model3d.camera", &view, "op", &doc).expect("camera");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].kind, OpKind::Command);
        assert_eq!(ops[0].payload.command(), "model3d.camera");
        match &ops[0].payload {
            Payload::Model3dCamera { camera: Some(c) } => {
                assert_eq!(c.fov, 45.0);
                assert_eq!(c.position, [0.0, 0.0, 11_713_743.0]);
            }
            other => panic!("{other:?}"),
        }

        // `null` is the reader's default view, not a relative edit.
        let ops = normalize(&km, st, "model3d.camera", &json!({"id": id, "camera": null}), "op-2", &doc).expect("reset");
        assert!(matches!(ops[0].payload, Payload::Model3dCamera { camera: None }));

        // A relative field, a degenerate view, a missing camera and a plain shape are all refused
        // on this replica, before any operation is sent.
        for bad in [
            json!({"id": id, "camera": {"position": [0.0, 0.0, 1.0], "target": [0.0, 0.0, 0.0], "up": [0.0, 1.0, 0.0], "fov": 45.0, "dx": 10.0}}),
            json!({"id": id, "camera": {"orbit": 15.0}}),
            json!({"id": id, "camera": {"position": [0.0, 0.0, 0.0], "target": [0.0, 0.0, 0.0], "up": [0.0, 1.0, 0.0], "fov": 45.0}}),
            json!({"id": id}),
            json!({"id": shapes[0].0, "camera": null}),
        ] {
            assert!(matches!(normalize(&km, st, "model3d.camera", &bad, "op-3", &doc), Err(LocalError::BadParams(_))), "{bad}");
        }

        // A viewer's camera step is refused on the client, before anything is queued.
        let mut viewer = CollabDoc::new("d");
        viewer.connect(&s, "jwt-v", ClientInfo { name: "viewer".into(), ver: "1".into() });
        let mut msg = welcome(0);
        if let ServerMsg::Welcome(ref mut welcome) = msg {
            welcome.role = deckcraft_collab_proto::Role::Viewer;
        }
        viewer.on_msg(&mut s, msg);
        assert_eq!(viewer.submit(&mut s, "model3d.camera", &view, "v-1"), Err(LocalError::ReadOnly));
    }

    #[test]
    fn a_model_scene_normalises_to_the_whole_final_value_and_a_relative_edit_is_refused() {
        let (mut s, doc, _slide, shapes) = deck();
        let data = base64::engine::general_purpose::STANDARD.encode(glb());
        let id = s.apply_remote("insert.model3d", &json!({"name": "block.glb", "data": data})).expect("model")["id"].as_u64().unwrap();
        let st = s.doc().expect("doc");
        let km = keys(st.doc.as_ref());

        // The whole final scene travels; the address is the model, the command is a value command.
        let value = json!({
            "id": id,
            "scene": {
                "light": {"type": "spot", "direction": [0.0, -1.0, -0.25], "intensity": 0.65, "angle": 27.0},
                "environment": {"ambient": 0.2, "background": [255, 239, 213]},
            },
        });
        let ops = normalize(&km, st, "model3d.scene", &value, "op", &doc).expect("scene");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].kind, OpKind::Command);
        assert_eq!(ops[0].payload.command(), "model3d.scene");
        match &ops[0].payload {
            Payload::Model3dScene { scene: Some(scene) } => {
                assert_eq!(scene.light.kind, deckcraft_collab_proto::ModelLightKind::Spot);
                assert_eq!(scene.light.direction, [0.0, -1.0, -0.25]);
                assert_eq!(scene.light.intensity, 0.65);
                assert_eq!(scene.light.angle, Some(27.0));
                assert_eq!(scene.environment.ambient, 0.2);
                assert_eq!(scene.environment.background, Some([255, 239, 213]));
            }
            other => panic!("{other:?}"),
        }

        // `null` is the reader's default scene, not a relative edit.
        let ops = normalize(&km, st, "model3d.scene", &json!({"id": id, "scene": null}), "op-2", &doc).expect("reset");
        assert!(matches!(ops[0].payload, Payload::Model3dScene { scene: None }));

        // A relative field, a light that cannot be drawn, a missing scene and a plain shape are
        // all refused on this replica, before any operation is sent.
        let light = |extra: Value| {
            let mut light = json!({"type": "point", "direction": [0.0, -1.0, 0.0], "intensity": 0.5});
            if let (Some(base), Some(extra)) = (light.as_object_mut(), extra.as_object()) {
                for (k, v) in extra {
                    base.insert(k.clone(), v.clone());
                }
            }
            light
        };
        for bad in [
            json!({"id": id, "scene": {"light": light(json!({})), "environment": {"ambient": 0.2}, "dimmer": 0.5}}),
            json!({"id": id, "scene": {"light": light(json!({"zoom": 2.0})), "environment": {"ambient": 0.2}}}),
            json!({"id": id, "scene": {"light": light(json!({"type": "spot"})), "environment": {"ambient": 0.2}}}),
            json!({"id": id, "scene": {"light": light(json!({"angle": 27.0})), "environment": {"ambient": 0.2}}}),
            json!({"id": id}),
            json!({"id": shapes[0].0, "scene": null}),
        ] {
            assert!(matches!(normalize(&km, st, "model3d.scene", &bad, "op-3", &doc), Err(LocalError::BadParams(_))), "{bad}");
        }

        // A viewer's scene step is refused on the client, before anything is queued.
        let mut viewer = CollabDoc::new("d");
        viewer.connect(&s, "jwt-v", ClientInfo { name: "viewer".into(), ver: "1".into() });
        let mut msg = welcome(0);
        if let ServerMsg::Welcome(ref mut welcome) = msg {
            welcome.role = deckcraft_collab_proto::Role::Viewer;
        }
        viewer.on_msg(&mut s, msg);
        assert_eq!(viewer.submit(&mut s, "model3d.scene", &value, "v-1"), Err(LocalError::ReadOnly));
    }

    #[test]
    fn a_video_command_uploads_bytes_and_sends_only_a_reference() {
        let (mut s, _doc, _slide, _shapes) = deck();
        let mut client = CollabDoc::new("d");
        client.connect(&s, "jwt", ClientInfo { name: "test".into(), ver: "1".into() });
        client.on_msg(&mut s, welcome(0));
        let bytes = mp4();
        let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
        client.submit(&mut s, "insert.video", &json!({"name": "clip.mp4", "data": data}), "op-v").expect("submit");
        let Some(ClientMsg::Op(op)) = client.next_outgoing() else { panic!("expected an operation") };
        match &op.payload {
            Payload::MediaInsert { kind, sha256, name, .. } => {
                assert_eq!(*kind, MediaKind::Video);
                assert_eq!(name, "clip.mp4");
                assert_eq!(sha256, &crate::media::sha256_hex(&bytes));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(op.payload.command(), "insert.video");
        assert_eq!(op.kind, OpKind::Structural);
        assert_eq!(client.take_uploads().len(), 1, "the bytes go to the media channel first");
    }

    #[test]
    fn a_model_command_uploads_the_glb_and_sends_only_a_reference() {
        let (mut s, _doc, _slide, _shapes) = deck();
        let mut client = CollabDoc::new("d");
        client.connect(&s, "jwt", ClientInfo { name: "test".into(), ver: "1".into() });
        client.on_msg(&mut s, welcome(0));
        let bytes = glb();
        let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
        client.submit(&mut s, "insert.model3d", &json!({"name": "block.glb", "data": data}), "op-m").expect("submit");
        let Some(ClientMsg::Op(op)) = client.next_outgoing() else { panic!("expected an operation") };
        match &op.payload {
            Payload::MediaInsert { kind, sha256, name, rect } => {
                assert_eq!(*kind, MediaKind::Model);
                assert_eq!(name, "block.glb");
                assert!(rect.is_none());
                assert_eq!(sha256, &crate::media::sha256_hex(&bytes));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(op.payload.command(), "insert.model3d");
        assert_eq!(op.kind, OpKind::Structural);
        assert!(!serde_json::to_string(&op.payload).expect("json").contains("data"), "no bytes on the wire");
        assert_eq!(client.take_uploads().len(), 1, "the model goes to the media channel first");
        assert!(client.has_media(&crate::media::sha256_hex(&bytes)), "the author keeps the bytes it uploaded");
    }

    #[test]
    fn a_fill_command_normalises_to_a_final_value() {
        let (mut s, doc, slide, shapes) = deck();
        let mut client = CollabDoc::new("d");
        client.connect(&s, "jwt", ClientInfo { name: "test".into(), ver: "1".into() });
        client.on_msg(&mut s, welcome(0));
        let km = keys(s.doc().expect("doc").doc.as_ref());
        let key = km.shape_key(slide, shapes[0]).expect("key").clone();
        let _ = doc;
        client.submit(&mut s, "shape.fill", &json!({"color": "accent1", "ids": [shapes[0].0]}), "op-f").expect("submit");
        let Some(ClientMsg::Op(op)) = client.next_outgoing() else { panic!("expected an operation") };
        assert_eq!(op.payload, Payload::ShapeFill { fill: FillSpec::Solid { color: "accent1".into() } });
        assert_eq!(op.target.shape.as_ref(), Some(&key));
    }

    /// A minimal ISO base media file: enough for the media allowlist, not for a decoder.
    fn mp4() -> Vec<u8> {
        let mut bytes = vec![0, 0, 0, 0x18];
        bytes.extend(b"ftypisom\0\0\x02\x00isomiso2");
        bytes.extend([0u8; 32]);
        bytes
    }

    /// A minimal binary glTF header: magic, version, declared length.
    fn glb() -> Vec<u8> {
        let mut bytes = b"glTF".to_vec();
        bytes.extend(2u32.to_le_bytes());
        bytes.extend(20u32.to_le_bytes());
        bytes.extend([0u8; 8]);
        bytes
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

    #[test]
    fn a_base_snapshot_rebuilds_the_replica_and_the_tail_finishes_it() {
        use deckcraft_collab_proto::{Capabilities, Limits, PROTO_VERSION, Role, SnapshotMode, Welcome};

        let (mut s, _doc, _slide, shapes) = deck();
        let presentation = s.doc().expect("doc").doc.as_ref().clone();
        let mut server =
            crate::DocSession::from_document("d", presentation, crate::SessionConfig { snapshot_every: 2, ..crate::SessionConfig::default() })
                .expect("session");
        let (slide_key, shape_key) = {
            let doc = server.document().expect("doc");
            let slide_ref = doc.slides.first().expect("slide");
            (server.keys().slide_key(slide_ref.id).expect("slide").clone(), server.keys().shape_key(slide_ref.id, shapes[0]).expect("shape").clone())
        };
        let move_to = |id: &str, x: f64, base: u64| OpEnvelope {
            op_id: id.into(),
            doc: "d".into(),
            base_version: base,
            kind: OpKind::Command,
            target: Target { slide: Some(slide_key.clone()), shape: Some(shape_key.clone()), ..Target::default() },
            payload: Payload::ShapeMove { x, y: 100.0 },
            history: None,
            preview: None,
        };
        for (i, x) in [200.0, 250.0, 300.0].iter().enumerate() {
            server.apply("alice", &move_to(&format!("a-{i}"), *x, i as u64)).expect("apply");
        }

        // The tail was compacted at version 2, so a client from nothing gets base@2 and the step
        // above it.
        let snapshot = server.resync(0);
        assert_eq!(snapshot.snapshot.mode, SnapshotMode::Base);
        assert!(snapshot.base.is_some());
        assert_eq!(snapshot.tail.len(), 1, "only the step above the base travels as a tail");

        let mut local = CollabDoc::new("d");
        let hello = local.connect(&s, "jwt", ClientInfo { name: "test".into(), ver: "1".into() });
        assert!(matches!(hello, ClientMsg::Hello(_)));
        local.on_msg(
            &mut s,
            ServerMsg::Welcome(Welcome {
                v: PROTO_VERSION,
                session_id: "d".into(),
                actor_id: "alice".into(),
                role: Role::Editor,
                lease: None,
                version: 3,
                snapshot: deckcraft_collab_proto::SnapshotRef { mode: SnapshotMode::BaseAndTail, cursor: "tail@0".into() },
                capabilities: Capabilities { kinds: vec![OpKind::Command], limits: Limits::default(), commands: vec![] },
            }),
        );
        assert!(matches!(local.next_outgoing(), Some(ClientMsg::Resync { last_version: 0 })));
        local.on_msg(&mut s, ServerMsg::Snapshot(snapshot));
        assert_eq!(local.last_apply_error(), None);
        assert_eq!(local.version(), 3, "the base plus its tail land on the head");
        assert_eq!(*s.doc().expect("doc").doc, *server.document().expect("doc"), "the replica matches the room");
        let moved = find_shape(&s.doc().expect("doc").doc.slides[0].shapes, shapes[0]).expect("shape");
        assert_eq!(moved.xfrm.expect("xfrm").x, 300.0, "the tail step applied on top of the base");
    }

    #[test]
    fn a_chained_base_rebuilds_the_replica_and_resumes_across_a_reconnect() {
        use deckcraft_collab_proto::{Capabilities, Limits, PROTO_VERSION, Role, SnapshotMode, Welcome};

        let (mut s, _doc, _slide, shapes) = deck();
        let presentation = s.doc().expect("doc").doc.as_ref().clone();
        let mut server = crate::DocSession::from_document(
            "d",
            presentation,
            crate::SessionConfig { snapshot_every: 2, max_base_inline: 1, max_base_chunk: 256, ..crate::SessionConfig::default() },
        )
        .expect("session");
        let (slide_key, shape_key) = {
            let doc = server.document().expect("doc");
            let slide_ref = doc.slides.first().expect("slide");
            (server.keys().slide_key(slide_ref.id).expect("slide").clone(), server.keys().shape_key(slide_ref.id, shapes[0]).expect("shape").clone())
        };
        let move_to = |id: &str, x: f64, base: u64| OpEnvelope {
            op_id: id.into(),
            doc: "d".into(),
            base_version: base,
            kind: OpKind::Command,
            target: Target { slide: Some(slide_key.clone()), shape: Some(shape_key.clone()), ..Target::default() },
            payload: Payload::ShapeMove { x, y: 100.0 },
            history: None,
            preview: None,
        };
        for (i, x) in [200.0, 250.0, 300.0].iter().enumerate() {
            server.apply("alice", &move_to(&format!("a-{i}"), *x, i as u64)).expect("apply");
        }
        let snapshot = server.resync(0);
        let chain = snapshot.base.as_ref().and_then(|base| base.chunks.clone()).expect("a chained base");
        assert!(chain.count > 1, "{} chunks", chain.count);
        let base_version = snapshot.base.as_ref().map(|base| base.version).expect("base version");

        let mut local = CollabDoc::new("d");
        let hello = local.connect(&s, "jwt", ClientInfo { name: "test".into(), ver: "1".into() });
        assert!(matches!(hello, ClientMsg::Hello(_)));
        local.on_msg(
            &mut s,
            ServerMsg::Welcome(Welcome {
                v: PROTO_VERSION,
                session_id: "d".into(),
                actor_id: "alice".into(),
                role: Role::Editor,
                lease: None,
                version: 3,
                snapshot: deckcraft_collab_proto::SnapshotRef { mode: SnapshotMode::BaseAndTail, cursor: "tail@0".into() },
                capabilities: Capabilities { kinds: vec![OpKind::Command], limits: Limits::default(), commands: vec![] },
            }),
        );
        assert!(matches!(local.next_outgoing(), Some(ClientMsg::Resync { last_version: 0 })));
        local.on_msg(&mut s, ServerMsg::Snapshot(snapshot));
        assert!(local.rebuilding(), "the replica is rebuilt from the chain, not from the frame");
        assert_eq!(local.version(), 0, "nothing is installed before the base is whole");
        assert!(matches!(local.submit(&mut s, "shape.move", &json!({"x": 10.0, "y": 10.0}), "local-1"), Err(LocalError::Rebuilding)));

        // One chunk arrives, then the socket drops: the chain resumes at the missing index.
        let mut chunks = 0;
        let Some(ClientMsg::BaseChunk { version, index }) = local.next_outgoing() else { panic!("a chunk request") };
        let (count, data) = server.base_chunk(version, index).expect("a chunk");
        local.on_msg(&mut s, ServerMsg::BaseChunk { version, index, count, data });
        chunks += 1;
        local.disconnected();
        let _ = local.connect(&s, "jwt", ClientInfo { name: "test".into(), ver: "1".into() });
        local.on_msg(
            &mut s,
            ServerMsg::Welcome(Welcome {
                v: PROTO_VERSION,
                session_id: "d".into(),
                actor_id: "alice".into(),
                role: Role::Editor,
                lease: None,
                version: 3,
                snapshot: deckcraft_collab_proto::SnapshotRef { mode: SnapshotMode::BaseAndTail, cursor: "tail@0".into() },
                capabilities: Capabilities { kinds: vec![OpKind::Command], limits: Limits::default(), commands: vec![] },
            }),
        );
        while let Some(msg) = local.next_outgoing() {
            match msg {
                ClientMsg::BaseChunk { version, index } => {
                    assert_eq!(version, base_version);
                    let (count, data) = server.base_chunk(version, index).expect("a chunk");
                    local.on_msg(&mut s, ServerMsg::BaseChunk { version, index, count, data });
                    chunks += 1;
                }
                ClientMsg::Resync { last_version } => {
                    let snapshot = server.resync(last_version);
                    local.on_msg(&mut s, ServerMsg::Snapshot(snapshot));
                }
                other => panic!("a rebuilding host sends {other:?}"),
            }
        }
        assert_eq!(chunks, chain.count, "every chunk arrived once, and none was asked for twice after the reconnect");
        assert_eq!(local.last_apply_error(), None);
        assert!(!local.rebuilding());
        assert_eq!(local.version(), 3, "the base plus its tail land on the head");
        assert_eq!(*s.doc().expect("doc").doc, *server.document().expect("doc"), "the replica matches the room");
        let moved = find_shape(&s.doc().expect("doc").doc.slides[0].shapes, shapes[0]).expect("shape");
        assert_eq!(moved.xfrm.expect("xfrm").x, 300.0, "the tail step applied on top of the assembled base");
    }

    /// The x of a shape's explicit box on the first slide.
    fn x_of(s: &Session, shape: ShapeId) -> f64 {
        find_shape(&s.doc().expect("doc").doc.slides[0].shapes, shape).expect("shape").xfrm.expect("box").x
    }

    /// A client that shows local edits before the room confirms them, connected and welcomed as
    /// an editor at version 0.
    fn previewing(s: &mut Session) -> CollabDoc {
        let mut local = CollabDoc::new("d");
        local.connect(s, "jwt", ClientInfo { name: "test".into(), ver: "1".into() });
        local.set_preview(true);
        local.on_msg(s, welcome(0));
        local
    }

    /// The room's answer for exactly the operation that went out.
    fn applied_for(op: &OpEnvelope, version: u64) -> ServerMsg {
        ServerMsg::Applied(Applied {
            op_id: op.op_id.clone(),
            version,
            actor_id: "alice".into(),
            command: Some(ConfirmedCommand { cmd: op.payload.command().into(), target: op.target.clone(), payload: op.payload.clone() }),
            delta: deckcraft_collab_proto::Delta::default(),
        })
    }

    #[test]
    fn a_previewed_move_is_visible_before_the_room_answers_and_settles_on_applied() {
        let (mut s, _doc, _slide, shapes) = deck();
        let mut local = previewing(&mut s);
        let start = x_of(&s, shapes[0]);
        let undo_steps = s.doc().expect("doc").history.undo.len();
        let revision = s.doc().expect("doc").revision;

        assert_eq!(local.submit(&mut s, "shape.move", &json!({"ids": [shapes[0].0], "x": 300.0, "y": 120.0}), "op-1"), Ok(1));
        assert!(local.is_previewing(), "the edit is on screen at once");
        assert_eq!(x_of(&s, shapes[0]), 300.0, "the local model moved before the answer");
        assert_eq!(local.version(), 0, "a preview moves no version");
        assert_eq!(s.doc().expect("doc").history.undo.len(), undo_steps, "a preview is not an undo step");
        assert!(s.doc().expect("doc").revision > revision, "the preview is a new revision, as an edit is");

        let Some(ClientMsg::Op(op)) = local.next_outgoing() else { panic!("the operation goes out") };
        assert_eq!(op.preview.as_deref(), Some("op-1"), "the wire carries the sender's local id");
        assert_eq!(local.on_msg(&mut s, applied_for(&op, 1)), Effect::Settled);
        assert!(!local.is_previewing(), "the answer takes the preview's place");
        assert_eq!(local.version(), 1);
        assert_eq!(x_of(&s, shapes[0]), 300.0, "the confirmed command is not applied a second time");
        assert_ne!(x_of(&s, shapes[0]), start);
    }

    #[test]
    fn a_refused_preview_rolls_back_exactly_that_edit_and_keeps_accepted_ones() {
        let (mut s, _doc, _slide, shapes) = deck();
        let mut local = previewing(&mut s);

        // One accepted step first, so the rollback has something it must not touch.
        local.submit(&mut s, "shape.move", &json!({"ids": [shapes[0].0], "x": 250.0, "y": 120.0}), "op-1").expect("submit");
        let Some(ClientMsg::Op(first)) = local.next_outgoing() else { panic!("the first operation") };
        local.on_msg(&mut s, applied_for(&first, 1));
        assert_eq!(x_of(&s, shapes[0]), 250.0);

        // A second one the room refuses before it applied it.
        local.submit(&mut s, "shape.move", &json!({"ids": [shapes[0].0], "x": 400.0, "y": 120.0}), "op-2").expect("submit");
        assert_eq!(x_of(&s, shapes[0]), 400.0, "the second edit is shown at once");
        let Some(ClientMsg::Op(second)) = local.next_outgoing() else { panic!("the second operation") };
        let rejected = ServerMsg::Rejected(deckcraft_collab_proto::Rejected {
            op_id: second.op_id.clone(),
            code: deckcraft_collab_proto::RejectCode::Invalid,
            message: "no such shape".into(),
            retryable: false,
            hint: Some(deckcraft_collab_proto::Hint { version: 1 }),
        });
        assert_eq!(local.on_msg(&mut s, rejected), Effect::Rollback);
        assert!(!local.is_previewing(), "the refused preview is gone");
        assert_eq!(x_of(&s, shapes[0]), 250.0, "exactly the refused edit is undone; the accepted one stays");
        assert_eq!(local.version(), 1, "a refusal moves no version");
        assert_eq!(local.client().queued(), 0, "a non-retryable refusal is not resent");
    }

    #[test]
    fn a_foreign_step_between_preview_and_applied_asks_for_the_tail_instead_of_mixing_orders() {
        use deckcraft_collab_proto::{SnapshotMode, SnapshotMsg, SnapshotRef};

        let (mut s, _doc, slide, shapes) = deck();
        let mut local = previewing(&mut s);
        let start = x_of(&s, shapes[0]);

        local.submit(&mut s, "shape.move", &json!({"ids": [shapes[0].0], "x": 300.0, "y": 120.0}), "op-1").expect("submit");
        assert_eq!(x_of(&s, shapes[0]), 300.0);
        let Some(ClientMsg::Op(op)) = local.next_outgoing() else { panic!("the operation") };

        // Someone else's step lands at version 1 while this replica's edit is unconfirmed. The
        // replica must not stack its preview on top of a step the room ordered before it.
        let other_key = local.keys().shape_key(slide, shapes[1]).expect("key").clone();
        let slide_key = local.keys().slide_key(slide).expect("slide").clone();
        let remote_payload = Payload::ShapeMove { x: 500.0, y: 120.0 };
        let remote_target = Target { slide: Some(slide_key.clone()), shape: Some(other_key.clone()), ..Target::default() };
        let remote = ServerMsg::Applied(Applied {
            op_id: "remote-1".into(),
            version: 1,
            actor_id: "bob".into(),
            command: Some(ConfirmedCommand { cmd: remote_payload.command().into(), target: remote_target.clone(), payload: remote_payload.clone() }),
            delta: deckcraft_collab_proto::Delta::default(),
        });
        assert_eq!(local.on_msg(&mut s, remote), Effect::Rollback);
        assert!(!local.is_previewing(), "the preview is abandoned, not merged");
        assert_eq!(x_of(&s, shapes[0]), start, "the local edit is back out of the model");
        assert_eq!(x_of(&s, shapes[1]), 400.0, "the foreign step is not applied on a document that already had the preview");
        assert_eq!(local.version(), 0, "but the applied step's version is rewound to the replica");
        assert!(
            matches!(local.next_outgoing(), Some(ClientMsg::Resync { last_version: 0 })),
            "the replica asks for the tail from the version it really holds"
        );

        // The tail carries the foreign step and this replica's operation in the room's order.
        let tail = vec![
            Applied {
                op_id: "remote-1".into(),
                version: 1,
                actor_id: "bob".into(),
                command: Some(ConfirmedCommand { cmd: remote_payload.command().into(), target: remote_target, payload: remote_payload }),
                delta: deckcraft_collab_proto::Delta::default(),
            },
            Applied {
                op_id: op.op_id.clone(),
                version: 2,
                actor_id: "alice".into(),
                command: Some(ConfirmedCommand { cmd: op.payload.command().into(), target: op.target.clone(), payload: op.payload.clone() }),
                delta: deckcraft_collab_proto::Delta::default(),
            },
        ];
        local.on_msg(
            &mut s,
            ServerMsg::Snapshot(SnapshotMsg {
                version: 2,
                snapshot: SnapshotRef { mode: SnapshotMode::BaseAndTail, cursor: "tail@0".into() },
                tail,
                base: None,
            }),
        );
        assert_eq!(x_of(&s, shapes[1]), 500.0, "the foreign step landed in the room's order");
        assert_eq!(x_of(&s, shapes[0]), 300.0, "and this replica's own step after it");
        assert_eq!(local.version(), 2);
        assert!(!local.is_previewing());
    }

    #[test]
    fn a_snapshot_that_does_not_cover_a_rewind_is_not_installed_by_the_host() {
        use deckcraft_collab_proto::{SnapshotMode, SnapshotMsg, SnapshotRef};

        let (mut s, _doc, _slide, shapes) = deck();
        let start = x_of(&s, shapes[0]);
        let mut local = previewing(&mut s);
        // A step that misses one asks for the tail, and the answer is on the wire from here.
        local.on_msg(&mut s, bare_applied("remote-2", 2));
        assert!(matches!(local.next_outgoing(), Some(ClientMsg::Resync { last_version: 0 })), "one resync is asked for");

        // An edit of this replica is shown while that request waits: a preview may start because
        // none of its own operations is on the wire.
        local.submit(&mut s, "shape.move", &json!({"ids": [shapes[0].0], "x": 300.0, "y": 120.0}), "op-1").expect("submit");
        assert!(local.is_previewing());

        // A foreign step lands at 4 while the preview is outstanding: the preview is put back and
        // the replica is rewound to the version it holds — with the resync already in flight, so
        // the answer that arrives will have been asked from above it.
        local.on_msg(&mut s, bare_applied("remote-4", 4));
        assert!(!local.is_previewing());
        assert_eq!(x_of(&s, shapes[0]), start, "the unconfirmed edit is out of the model again");
        assert_eq!(local.version(), 0);

        // The room cannot serve that tail (it was compacted) and cannot build a base either, so it
        // answers with an empty tail at a head above the rewind. There is nothing in that answer
        // the replica could hold, and applying it would make the host claim version 4 while steps
        // 1..3 are missing — the tail that follows would then be skipped as already seen.
        local.on_msg(
            &mut s,
            ServerMsg::Snapshot(SnapshotMsg {
                version: 4,
                snapshot: SnapshotRef { mode: SnapshotMode::BaseAndTail, cursor: "tail@4".into() },
                tail: vec![],
                base: None,
            }),
        );
        assert_eq!(local.version(), 0, "a refused snapshot moved the host's version");
        assert!(
            matches!(local.next_outgoing(), Some(ClientMsg::Resync { last_version: 0 })),
            "the tail is asked for again from the version the replica holds"
        );
    }

    /// A step that carries no command: tests that care about the order of versions, not documents.
    fn bare_applied(op_id: &str, version: u64) -> ServerMsg {
        ServerMsg::Applied(Applied {
            op_id: op_id.into(),
            version,
            actor_id: "bob".into(),
            command: None,
            delta: deckcraft_collab_proto::Delta::default(),
        })
    }

    #[test]
    fn a_viewer_or_an_offline_replica_shows_nothing_before_it_is_confirmed() {
        let (mut s, _doc, _slide, shapes) = deck();
        let start = x_of(&s, shapes[0]);

        // A viewer is refused before anything is queued and nothing moves.
        let mut viewer = CollabDoc::new("d");
        viewer.connect(&s, "jwt-v", ClientInfo { name: "viewer".into(), ver: "1".into() });
        viewer.set_preview(true);
        let mut w = welcome(0);
        if let ServerMsg::Welcome(ref mut welcome) = w {
            welcome.role = Role::Viewer;
        }
        viewer.on_msg(&mut s, w);
        assert_eq!(viewer.submit(&mut s, "shape.move", &json!({"ids": [shapes[0].0], "x": 300.0, "y": 120.0}), "v-1"), Err(LocalError::ReadOnly));
        assert!(!viewer.is_previewing());
        assert_eq!(x_of(&s, shapes[0]), start);

        // An editor whose link is not live queues the operation and shows nothing.
        let mut offline = CollabDoc::new("d");
        offline.set_preview(true);
        offline.connect(&s, "jwt", ClientInfo { name: "test".into(), ver: "1".into() });
        assert_eq!(offline.submit(&mut s, "shape.move", &json!({"ids": [shapes[0].0], "x": 300.0, "y": 120.0}), "o-1"), Ok(1));
        assert!(!offline.is_previewing());
        assert_eq!(x_of(&s, shapes[0]), start, "nothing is shown before the handshake");
        assert!(offline.next_outgoing().is_none(), "nothing goes out before the handshake either");
    }

    #[test]
    fn a_command_after_a_preview_waits_for_confirmed_state() {
        let (mut s, _doc, _slide, shapes) = deck();
        let mut local = previewing(&mut s);

        local.submit(&mut s, "shape.move", &json!({"ids": [shapes[0].0], "x": 300.0, "y": 120.0}), "op-1").expect("submit");
        assert!(local.is_previewing());
        // A second command during the preview is held: normalising it now would read an edit only
        // this replica has.
        assert_eq!(local.submit(&mut s, "shape.fill", &json!({"color": "#00ff00", "ids": [shapes[1].0]}), "op-2"), Ok(0));
        assert_eq!(local.deferred(), 1);
        let Some(ClientMsg::Op(first)) = local.next_outgoing() else { panic!("only the first operation goes out") };
        assert_eq!(first.op_id, "op-1");
        assert!(local.next_outgoing().is_none(), "the second command waits for confirmed state");

        local.on_msg(&mut s, applied_for(&first, 1));
        assert_eq!(local.deferred(), 0, "the held command is normalised once the answer arrives");
        assert!(local.is_previewing(), "and is itself shown at once");
        let filled = find_shape(&s.doc().expect("doc").doc.slides[0].shapes, shapes[1]).expect("shape");
        assert!(filled.fill.is_some(), "the second edit is visible before its answer");
        let Some(ClientMsg::Op(second)) = local.next_outgoing() else { panic!("the second operation") };
        assert_eq!(second.op_id, "op-2");
        local.on_msg(&mut s, applied_for(&second, 2));
        assert_eq!(local.version(), 2);
        assert!(!local.is_previewing());
    }

    #[test]
    fn preview_is_off_by_default_and_the_wire_carries_no_local_id() {
        let (mut s, _doc, _slide, shapes) = deck();
        let mut local = CollabDoc::new("d");
        local.connect(&s, "jwt", ClientInfo { name: "test".into(), ver: "1".into() });
        local.on_msg(&mut s, welcome(0));
        assert!(!local.preview_enabled());
        assert_eq!(local.submit(&mut s, "shape.move", &json!({"ids": [shapes[0].0], "x": 300.0, "y": 120.0}), "op-1"), Ok(1));
        assert!(!local.is_previewing(), "nothing is shown before the answer");
        let Some(ClientMsg::Op(op)) = local.next_outgoing() else { panic!("the operation") };
        assert_eq!(op.preview, None);
    }
}
