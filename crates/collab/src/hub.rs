//! The session's message loop: who is in the room, what each may do, and which answers go to
//! whom. No socket lives here — the transport authenticates the handshake, then hands messages
//! to [`Hub`] and pumps the per-peer outbox back onto the wire.
//!
//! This is the piece a server binary and the headless tests share. Rules it enforces:
//!
//! - a viewer's `op` never reaches the engine (ADR 0008);
//! - an `applied` goes to every participant, with the command the server actually ran, so every
//!   replica can run the same command in the same order and converge;
//! - presence goes to everyone **except** its author and never touches `version` (ADR 0006);
//! - a repeated `opId` returns the stored answer and changes nothing (ADR 0003).

use std::collections::{BTreeMap, HashMap, VecDeque};

use base64::Engine as _;
use deckcraft_collab_proto::{
    ClientInfo, ClientMsg, CommitResult, Hello, PROTO_VERSION, RejectCode, Rejected, Role, ServerError, ServerMsg, SnapshotMode, SnapshotRef, Welcome,
};
use sha2::{Digest, Sha256};

use crate::session::{CommitError, DocSession};

/// Who the transport authenticated. The token is never stored here (ADR 0008).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claims {
    pub actor_id: String,
    pub role: Role,
}

/// One participant of the room.
#[derive(Clone, Debug)]
pub struct Peer {
    pub role: Role,
    pub client: ClientInfo,
}

/// One document room with its participants and outboxes.
pub struct Hub {
    session: DocSession,
    peers: BTreeMap<String, Peer>,
    out: HashMap<String, VecDeque<ServerMsg>>,
    last_applied: Option<deckcraft_collab_proto::Applied>,
    /// Bytes of the last base a `resync` sent, for the server's counters.
    last_base_transfer: Option<u64>,
    /// Bytes of the last base chunk handed out, for the server's counters.
    last_base_chunk: Option<u64>,
}

impl Hub {
    pub fn new(session: DocSession) -> Self {
        Hub { session, peers: BTreeMap::new(), out: HashMap::new(), last_applied: None, last_base_transfer: None, last_base_chunk: None }
    }

    pub fn session(&self) -> &DocSession {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut DocSession {
        &mut self.session
    }

    pub fn version(&self) -> u64 {
        self.session.version()
    }

    pub fn file_id(&self) -> &str {
        self.session.file_id()
    }

    /// Store picture bytes uploaded over the media channel. The digest is the object's name.
    pub fn put_media(&mut self, name: &str, bytes: Vec<u8>) -> Result<String, crate::MediaError> {
        self.session.put_media(name, bytes)
    }

    /// Picture bytes this room holds, for `GET /media/{sha256}` and for a client that is about
    /// to apply a `picture.insert`.
    pub fn media(&self, sha: &str) -> Option<std::sync::Arc<Vec<u8>>> {
        self.session.media(sha)
    }

    /// Participants in a stable order, with the role the server enforces.
    pub fn participants(&self) -> Vec<(String, Role)> {
        self.peers.iter().map(|(actor, peer)| (actor.clone(), peer.role)).collect()
    }

    /// Admit an authenticated client and return its `welcome`. The caller sends it back over the
    /// socket; the hub also queues it, so a transport that pumps the outbox needs no special case.
    pub fn join(&mut self, hello: &Hello, claims: Claims) -> Result<Welcome, ServerError> {
        if hello.v != PROTO_VERSION {
            return Err(ServerError {
                code: "unsupported".into(),
                message: format!("protocol version {} is not supported (this server speaks {PROTO_VERSION})", hello.v),
                retryable: false,
            });
        }
        if hello.app != "deckcraft" {
            return Err(ServerError { code: "invalid".into(), message: "this room serves DeckCraft documents".into(), retryable: false });
        }
        if hello.doc != self.session.file_id() {
            return Err(ServerError { code: "invalid".into(), message: "the document does not match the session".into(), retryable: false });
        }
        let welcome = Welcome {
            v: PROTO_VERSION,
            session_id: self.session.file_id().to_string(),
            actor_id: claims.actor_id.clone(),
            role: claims.role,
            lease: None,
            version: self.session.version(),
            snapshot: SnapshotRef { mode: SnapshotMode::BaseAndTail, cursor: format!("tail@{}", self.session.version()) },
            capabilities: deckcraft_collab_proto::Capabilities {
                kinds: vec![deckcraft_collab_proto::OpKind::Command, deckcraft_collab_proto::OpKind::Structural],
                limits: deckcraft_collab_proto::Limits::default(),
                commands: vec![
                    "shape.move".into(),
                    "shape.setBounds".into(),
                    "shape.setBoundsMany".into(),
                    "text.set".into(),
                    "text.insert".into(),
                    "text.delete".into(),
                    "table.edit".into(),
                    "table.style".into(),
                    "table.options".into(),
                    "table.cellFill".into(),
                    "table.merge".into(),
                    "table.split".into(),
                    "table.columnWidth".into(),
                    "table.rowHeight".into(),
                    "smartArt.setNodes".into(),
                    "smartArt.addNode".into(),
                    "smartArt.removeNode".into(),
                    "smartArt.promote".into(),
                    "smartArt.demote".into(),
                    "smartArt.layout".into(),
                    "smartArt.style".into(),
                    "smartArt.colors".into(),
                    "smartArt.nodes".into(),
                    "smartArt.picture".into(),
                    "insert.smartArt".into(),
                    "smartArt.fromText".into(),
                    "smartArt.toText".into(),
                    "smartArt.toShapes".into(),
                    "insert.chart".into(),
                    "chart.type".into(),
                    "chart.data".into(),
                    "chart.options".into(),
                    "model3d.camera".into(),
                    "model3d.scene".into(),
                    "shape.insert".into(),
                    "insert.picture".into(),
                    "edit.delete".into(),
                    "slide.new".into(),
                    "slide.delete".into(),
                    "slide.move".into(),
                    "slide.transition.set".into(),
                    "slide.animation.set".into(),
                    "slide.animations.set".into(),
                    "arrange.group".into(),
                    "arrange.ungroup".into(),
                    "arrange.reorder".into(),
                    "font.create".into(),
                    "font.set".into(),
                    "font.remove".into(),
                ],
            },
        };
        self.peers.insert(claims.actor_id.clone(), Peer { role: claims.role, client: hello.client.clone() });
        self.out.entry(claims.actor_id).or_default();
        Ok(welcome)
    }

    /// Handle one message from a connected peer. The answer, if any, is queued in the outbox.
    pub fn receive(&mut self, actor: &str, msg: ClientMsg) -> Result<(), ServerError> {
        let Some(peer) = self.peers.get(actor).cloned() else {
            return Err(ServerError { code: "auth".into(), message: "join first".into(), retryable: false });
        };
        match msg {
            ClientMsg::Hello(_) => Err(ServerError { code: "invalid".into(), message: "already joined".into(), retryable: false }),
            ClientMsg::Op(op) => {
                if !peer.role.may_write() {
                    self.send(
                        actor,
                        ServerMsg::Rejected(Rejected {
                            op_id: op.op_id,
                            code: RejectCode::Forbidden,
                            message: "a viewer cannot change the document".into(),
                            retryable: false,
                            hint: None,
                        }),
                    );
                    return Ok(());
                }
                match self.session.apply(actor, &op) {
                    Ok(applied) => {
                        self.last_applied = Some(applied.clone());
                        self.broadcast(ServerMsg::Applied(applied))
                    }
                    Err(rejected) => self.send(actor, ServerMsg::Rejected(rejected)),
                }
                Ok(())
            }
            ClientMsg::Presence(mut presence) => {
                // The author is the token's subject, never a field the client fills in.
                presence.actor_id = actor.to_string();
                let msg = ServerMsg::Presence(presence);
                let others: Vec<String> = self.peers.keys().filter(|other| other.as_str() != actor).cloned().collect();
                for other in others {
                    self.send(&other, msg.clone());
                }
                Ok(())
            }
            ClientMsg::Resync { last_version } => {
                let snapshot = self.session.resync(last_version);
                if let Some(base) = &snapshot.base {
                    self.last_base_transfer = Some(base.bytes_base64.as_ref().map_or(0, |bytes| bytes.len() as u64));
                }
                self.send(actor, ServerMsg::Snapshot(snapshot));
                Ok(())
            }
            ClientMsg::BaseChunk { version, index } => {
                // A base chunk is read-only: like a query it moves no version and a viewer may ask
                // for it (a viewer whose journal was compacted still has to rebuild its replica).
                match self.session.base_chunk(version, index) {
                    Ok((count, data)) => {
                        self.last_base_chunk = Some(data.len() as u64);
                        self.send(actor, ServerMsg::BaseChunk { version, index, count, data });
                    }
                    Err(error) => {
                        // `stale` is retryable: the client drops the chain and asks for a new
                        // snapshot. The refusal is answered like any other, so the client's own
                        // retry rule decides.
                        self.send(
                            actor,
                            ServerMsg::Rejected(Rejected {
                                op_id: format!("base@{version}/{index}"),
                                code: if error.retryable { RejectCode::Stale } else { RejectCode::Invalid },
                                message: error.message,
                                retryable: error.retryable,
                                hint: None,
                            }),
                        );
                    }
                }
                Ok(())
            }
            ClientMsg::Undo => self.history(actor, true),
            ClientMsg::Redo => self.history(actor, false),
            ClientMsg::Query { id, cmd, target, params } => {
                // A query is read-only: a viewer may ask it, it moves no version and it enters
                // neither the journal nor anyone's history. The answer goes to the asker alone.
                let version = self.session.version();
                let answer = match self.session.query(&cmd, &target, &params) {
                    Ok(result) => ServerMsg::QueryResult { id, version, result: Some(result), error: None },
                    Err(error) => ServerMsg::QueryResult { id, version, result: None, error: Some(error) },
                };
                self.send(actor, answer);
                Ok(())
            }
            ClientMsg::Ping => {
                self.send(actor, ServerMsg::Pong);
                Ok(())
            }
            ClientMsg::Commit { expected_file_version } => {
                if !peer.role.may_write() {
                    self.send(
                        actor,
                        ServerMsg::Rejected(Rejected {
                            op_id: "commit".into(),
                            code: RejectCode::Forbidden,
                            message: "a viewer cannot commit".into(),
                            retryable: false,
                            hint: None,
                        }),
                    );
                    return Ok(());
                }
                self.commit(actor, expected_file_version.as_deref());
                Ok(())
            }
        }
    }

    /// Build and apply this actor's undo or redo. The server owns the before/after image; the
    /// resulting inverse is an ordinary operation, broadcast like any other (ADR 0010).
    fn history(&mut self, actor: &str, undo: bool) -> Result<(), ServerError> {
        if self.peers.get(actor).is_none_or(|peer| !peer.role.may_write()) {
            self.send(
                actor,
                ServerMsg::Rejected(Rejected {
                    op_id: "undo".into(),
                    code: RejectCode::Forbidden,
                    message: "a viewer has no history to move".into(),
                    retryable: false,
                    hint: None,
                }),
            );
            return Ok(());
        }
        let id = format!("{actor}-{}-{}", if undo { "undo" } else { "redo" }, self.session.version());
        let built = if undo { self.session.undo(actor, &id) } else { self.session.redo(actor, &id) };
        let op = match built {
            Ok(op) => op,
            Err(rej) => {
                let rejected = Rejected {
                    op_id: id,
                    code: rej.code,
                    message: rej.message,
                    retryable: rej.retryable,
                    hint: rej.hint_version.map(|version| deckcraft_collab_proto::Hint { version }),
                };
                self.send(actor, ServerMsg::Rejected(rejected));
                return Ok(());
            }
        };
        match self.session.apply(actor, &op) {
            Ok(applied) => {
                self.last_applied = Some(applied.clone());
                self.broadcast(ServerMsg::Applied(applied))
            }
            Err(rejected) => self.send(actor, ServerMsg::Rejected(rejected)),
        }
        Ok(())
    }

    /// The last operation this room accepted, for the journal. Taking it clears the slot.
    pub fn take_last_applied(&mut self) -> Option<deckcraft_collab_proto::Applied> {
        self.last_applied.take()
    }

    /// The size of the last base a `resync` sent (inline bytes; `0` for a cursor without bytes).
    pub fn take_last_base_transfer(&mut self) -> Option<u64> {
        self.last_base_transfer.take()
    }

    /// The encoded size of the last base chunk handed out, for the server's counters.
    pub fn take_last_base_chunk(&mut self) -> Option<u64> {
        self.last_base_chunk.take()
    }

    /// Build the file and queue a `commit-result`. Errors become a `rejected` for the requester;
    /// a conflict is the cabinet's `409` (ADR 0009).
    pub fn commit(&mut self, actor: &str, expected: Option<&str>) {
        match self.session.commit_checked(expected) {
            Ok(done) => {
                let sha = hex(&Sha256::digest(&done.bytes));
                let result = CommitResult {
                    sha256: sha,
                    version: done.version,
                    file_version: done.file_version,
                    bytes_base64: Some(base64::engine::general_purpose::STANDARD.encode(&done.bytes)),
                    blob: None,
                    warnings: done.warnings,
                };
                self.send(actor, ServerMsg::CommitResult(result));
            }
            Err(CommitError::Conflict { expected, current }) => self.send(
                actor,
                ServerMsg::Rejected(Rejected {
                    op_id: "commit".into(),
                    code: RejectCode::Conflict,
                    message: match current {
                        Some(current) => format!("the file changed outside the session: expected {expected}, current {current}"),
                        None => format!("the file changed outside the session: expected {expected}"),
                    },
                    retryable: false,
                    hint: None,
                }),
            ),
            Err(CommitError::Build(message)) => self.send(actor, ServerMsg::Error(ServerError { code: "commit".into(), message, retryable: true })),
        }
    }

    /// Tell the session which file version the cabinet stored after a commit.
    pub fn accept_file_version(&mut self, etag: impl Into<String>) {
        self.session.accept_file_version(etag);
    }

    /// Whether this participant may build a commit. A viewer may not (ADR-0008).
    pub fn may_commit(&self, actor: &str) -> bool {
        self.peers.get(actor).is_some_and(|peer| peer.role.may_write())
    }

    /// Queue one message for a peer. A transport that publishes a commit to the cabinet outside
    /// the room lock answers with this after the cabinet took the file (ADR-0009).
    pub fn answer(&mut self, actor: &str, msg: ServerMsg) {
        self.send(actor, msg);
    }

    /// Remove a peer (socket closed). Its queued answers go too.
    pub fn remove_peer(&mut self, actor: &str) {
        self.peers.remove(actor);
        self.out.remove(actor);
    }

    /// The next message for one peer, if any.
    pub fn poll(&mut self, actor: &str) -> Option<ServerMsg> {
        self.out.get_mut(actor)?.pop_front()
    }

    pub fn queued(&self, actor: &str) -> usize {
        self.out.get(actor).map_or(0, VecDeque::len)
    }

    fn send(&mut self, actor: &str, msg: ServerMsg) {
        self.out.entry(actor.to_string()).or_default().push_back(msg);
    }

    fn broadcast(&mut self, msg: ServerMsg) {
        for actor in self.peers.keys().cloned().collect::<Vec<_>>() {
            self.send(&actor, msg.clone());
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::CollabDoc;
    use deckcraft_collab_proto::{ClientInfo, OpEnvelope, OpKind, Payload, ShapeKey, SlideKey, SnapshotMsg, Target};
    use deckcraft_engine::Session;
    use deckcraft_model::Presentation;
    use serde_json::json;

    fn deck_session() -> Session {
        let mut s = Session::with_new();
        let title = s.doc().expect("doc").doc.slides[0].id;
        s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("blank");
        s.apply_remote("slide.delete", &json!({"ids": [title.0]})).expect("drop title");
        s.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).expect("rect");
        s.apply_remote("shape.insert", &json!({"preset": "ellipse", "rect": [400, 100, 120, 100]})).expect("ellipse");
        s
    }

    fn deck_doc() -> Presentation {
        deck_session().doc().expect("doc").doc.as_ref().clone()
    }

    fn hello() -> Hello {
        Hello {
            v: PROTO_VERSION,
            app: "deckcraft".into(),
            doc: "d".into(),
            jwt: "jwt".into(),
            client: ClientInfo { name: "test".into(), ver: "1".into() },
            last_version: Some(0),
        }
    }

    fn hub() -> Hub {
        let mut session = DocSession::from_document("d", deck_doc(), Default::default()).expect("session");
        session.set_base_file_etag("etag-0");
        Hub::new(session)
    }

    fn join(hub: &mut Hub, actor: &str, role: Role) -> Welcome {
        hub.join(&hello(), Claims { actor_id: actor.into(), role }).expect("join")
    }

    fn keys_of(hub: &Hub) -> (SlideKey, ShapeKey) {
        let doc = hub.session().document().expect("doc");
        let slide = hub.session().keys().slide_key(doc.slides[0].id).expect("slide").clone();
        let shape = hub.session().keys().shape_key(doc.slides[0].id, doc.slides[0].shapes[0].id).expect("shape").clone();
        (slide, shape)
    }

    fn move_op(id: &str, base: u64, slide: &SlideKey, shape: &ShapeKey, x: f64) -> OpEnvelope {
        OpEnvelope {
            op_id: id.into(),
            doc: "d".into(),
            base_version: base,
            kind: OpKind::Command,
            target: Target { slide: Some(slide.clone()), shape: Some(shape.clone()), ..Target::default() },
            payload: Payload::ShapeMove { x, y: 100.0 },
            history: None,
            preview: None,
        }
    }

    #[test]
    fn an_operation_reaches_every_participant_once() {
        let mut hub = hub();
        let _ = join(&mut hub, "alice", Role::Editor);
        let _ = join(&mut hub, "bob", Role::Editor);
        let (slide, shape) = keys_of(&hub);
        hub.receive("alice", ClientMsg::Op(move_op("op-1", 0, &slide, &shape, 250.0))).expect("receive");
        let alice = hub.poll("alice").expect("answer");
        let bob = hub.poll("bob").expect("broadcast");
        assert_eq!(alice, bob, "the same authoritative command goes to everyone");
        match alice {
            ServerMsg::Applied(a) => assert_eq!(a.version, 1),
            other => panic!("{other:?}"),
        }
        assert_eq!(hub.version(), 1);
        // A resend is deduplicated: nothing applies twice and the stored answer comes back.
        hub.receive("alice", ClientMsg::Op(move_op("op-1", 0, &slide, &shape, 250.0))).expect("resend");
        match hub.poll("alice").expect("dup") {
            ServerMsg::Applied(a) => assert_eq!(a.version, 1),
            other => panic!("{other:?}"),
        }
        assert_eq!(hub.version(), 1);
    }

    #[test]
    fn a_viewer_is_refused_before_the_engine() {
        let mut hub = hub();
        let _ = join(&mut hub, "victor", Role::Viewer);
        let before = hub.session().document().expect("doc").clone();
        let (slide, shape) = keys_of(&hub);
        hub.receive("victor", ClientMsg::Op(move_op("op-1", 0, &slide, &shape, 1.0))).expect("receive");
        match hub.poll("victor").expect("answer") {
            ServerMsg::Rejected(r) => assert_eq!(r.code, RejectCode::Forbidden),
            other => panic!("{other:?}"),
        }
        assert_eq!(*hub.session().document().expect("doc"), before);
        assert_eq!(hub.version(), 0);
    }

    #[test]
    fn presence_goes_to_the_others_and_never_becomes_a_version() {
        let mut hub = hub();
        let _ = join(&mut hub, "alice", Role::Editor);
        let _ = join(&mut hub, "bob", Role::Editor);
        let msg = deckcraft_collab_proto::PresenceMsg {
            actor_id: "spoofed".into(),
            slide: SlideKey("s".into()),
            shapes: vec![ShapeKey("h".into())],
            text: None,
            viewport: None,
            color: None,
        };
        hub.receive("alice", ClientMsg::Presence(msg)).expect("receive");
        assert!(hub.poll("alice").is_none(), "presence is not echoed to its author");
        match hub.poll("bob").expect("bob sees it") {
            ServerMsg::Presence(p) => assert_eq!(p.actor_id, "alice", "the server names the author"),
            other => panic!("{other:?}"),
        }
        assert_eq!(hub.version(), 0);
    }

    #[test]
    fn a_viewer_may_ask_for_a_base_chunk_and_a_moved_base_is_refused_as_stale() {
        let session = DocSession::from_document(
            "d",
            deck_doc(),
            crate::session::SessionConfig { snapshot_every: 1, max_base_inline: 1, max_base_chunk: 256, ..crate::session::SessionConfig::default() },
        )
        .expect("session");
        let mut hub = Hub::new(session);
        let _ = join(&mut hub, "victor", Role::Viewer);
        let (slide, shape) = keys_of(&hub);
        // A step so there is something to compact, then a resync from zero: a base is needed.
        assert!(hub.session_mut().apply("alice", &move_op("op-1", 0, &slide, &shape, 250.0)).is_ok());
        hub.receive("victor", ClientMsg::Resync { last_version: 0 }).expect("resync");
        let Some(ServerMsg::Snapshot(snapshot)) = hub.poll("victor") else { panic!("a snapshot") };
        let base = snapshot.base.expect("a base");
        let chain = base.chunks.expect("the base travels as a chain");
        assert!(chain.count > 1, "{} chunks", chain.count);

        // A viewer rebuilding its replica is a reader: asking for a chunk is allowed.
        hub.receive("victor", ClientMsg::BaseChunk { version: base.version, index: 0 }).expect("chunk");
        match hub.poll("victor").expect("the chunk") {
            ServerMsg::BaseChunk { version, index, count, data } => {
                assert_eq!((version, index), (base.version, 0));
                assert_eq!(count, chain.count);
                assert_eq!(data.len(), chain.chunk_len(0).expect("length").div_ceil(3) * 4, "base64 of one chunk");
            }
            other => panic!("{other:?}"),
        }
        assert!(hub.take_last_base_chunk().is_some(), "the server counts chunks");

        // A base compaction replaced is not served: the client is told to ask again.
        hub.receive("victor", ClientMsg::BaseChunk { version: base.version + 1, index: 0 }).expect("chunk");
        match hub.poll("victor").expect("the refusal") {
            ServerMsg::Rejected(rejection) => {
                assert_eq!(rejection.code, RejectCode::Stale);
                assert!(rejection.retryable, "a fresh snapshot is the fix");
                assert_eq!(rejection.op_id, format!("base@{}/0", base.version + 1));
            }
            other => panic!("{other:?}"),
        }
        hub.receive("victor", ClientMsg::BaseChunk { version: base.version, index: chain.count }).expect("chunk");
        match hub.poll("victor").expect("the refusal") {
            ServerMsg::Rejected(rejection) => {
                assert_eq!(rejection.code, RejectCode::Invalid);
                assert!(!rejection.retryable);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(hub.version(), 1, "asking for a chunk moves no version");
    }

    #[test]
    fn a_resync_replays_the_tail_after_the_client_version() {
        let mut hub = hub();
        let _ = join(&mut hub, "alice", Role::Editor);
        let (slide, shape) = keys_of(&hub);
        for (i, x) in [200.0, 300.0].iter().enumerate() {
            hub.receive("alice", ClientMsg::Op(move_op(&format!("op-{i}"), i as u64, &slide, &shape, *x))).expect("receive");
            let _ = hub.poll("alice");
        }
        hub.receive("alice", ClientMsg::Resync { last_version: 1 }).expect("resync");
        match hub.poll("alice").expect("snapshot") {
            ServerMsg::Snapshot(SnapshotMsg { version, tail, .. }) => {
                assert_eq!(version, 2);
                assert_eq!(tail.len(), 1);
                assert_eq!(tail[0].version, 2);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn commit_returns_the_file_and_a_stale_file_version_is_a_conflict() {
        let mut hub = hub();
        let _ = join(&mut hub, "owner", Role::Owner);
        hub.receive("owner", ClientMsg::Commit { expected_file_version: Some("etag-0".into()) }).expect("commit");
        match hub.poll("owner").expect("result") {
            ServerMsg::CommitResult(r) => {
                assert_eq!(r.version, 0);
                assert!(!r.sha256.is_empty());
                let bytes = base64::engine::general_purpose::STANDARD.decode(r.bytes_base64.expect("bytes")).expect("decode");
                assert!(bytes.starts_with(b"PK"), "a .pptx is a zip");
            }
            other => panic!("{other:?}"),
        }
        hub.receive("owner", ClientMsg::Commit { expected_file_version: Some("etag-old".into()) }).expect("commit");
        match hub.poll("owner").expect("conflict") {
            ServerMsg::Rejected(r) => assert_eq!(r.code, RejectCode::Conflict),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn undo_is_an_operation_and_never_takes_back_someone_elses_step() {
        let mut hub = hub();
        let _ = join(&mut hub, "alice", Role::Editor);
        let _ = join(&mut hub, "bob", Role::Editor);
        let doc = hub.session().document().expect("doc");
        let slide = hub.session().keys().slide_key(doc.slides[0].id).expect("slide").clone();
        let first = hub.session().keys().shape_key(doc.slides[0].id, doc.slides[0].shapes[0].id).expect("a").clone();
        let second = hub.session().keys().shape_key(doc.slides[0].id, doc.slides[0].shapes[1].id).expect("b").clone();
        hub.receive("alice", ClientMsg::Op(move_op("a1", 0, &slide, &first, 250.0))).expect("a");
        let _ = hub.poll("alice");
        let _ = hub.poll("bob");
        hub.receive("bob", ClientMsg::Op(move_op("b1", 1, &slide, &second, 500.0))).expect("b");
        let _ = hub.poll("alice");
        let _ = hub.poll("bob");
        hub.receive("alice", ClientMsg::Undo).expect("undo");
        match hub.poll("alice").expect("applied") {
            ServerMsg::Applied(a) => assert_eq!(a.version, 3),
            other => panic!("{other:?}"),
        }
        let doc = hub.session().document().expect("doc");
        assert_eq!(doc.slides[0].shapes[0].xfrm.expect("xfrm").x, 100.0, "alice's move is taken back");
        assert_eq!(doc.slides[0].shapes[1].xfrm.expect("xfrm").x, 500.0, "bob's move stays");
        assert_eq!(hub.version(), 3);

        let _ = join(&mut hub, "victor", Role::Viewer);
        hub.receive("victor", ClientMsg::Undo).expect("undo");
        match hub.poll("victor").expect("refused") {
            ServerMsg::Rejected(r) => assert_eq!(r.code, RejectCode::Forbidden),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn two_typing_clients_keep_both_insertions() {
        let mut hub = hub();
        let _ = join(&mut hub, "alice", Role::Editor);
        let _ = join(&mut hub, "bob", Role::Editor);
        let (slide, shape) = keys_of(&hub);
        let set = OpEnvelope {
            op_id: "set".into(),
            doc: "d".into(),
            base_version: 0,
            kind: OpKind::Command,
            target: Target { slide: Some(slide.clone()), shape: Some(shape.clone()), ..Target::default() },
            payload: Payload::TextSet { text: "AB".into(), cell: None },
            history: None,
            preview: None,
        };
        hub.receive("alice", ClientMsg::Op(set)).expect("set");
        let _ = hub.poll("alice");
        let _ = hub.poll("bob");
        for (id, ch) in [("a", "X"), ("b", "Y")] {
            let op = OpEnvelope {
                op_id: id.into(),
                doc: "d".into(),
                base_version: 1,
                kind: OpKind::Command,
                target: Target { slide: Some(slide.clone()), shape: Some(shape.clone()), ..Target::default() },
                payload: Payload::TextInsert { at: 1, text: ch.into(), cell: None },
                history: None,
                preview: None,
            };
            hub.receive(if id == "a" { "alice" } else { "bob" }, ClientMsg::Op(op)).expect("insert");
            let _ = hub.poll("alice");
            let _ = hub.poll("bob");
        }
        let doc = hub.session().document().expect("doc");
        let text = doc.slides[0].shapes[0].text.as_ref().expect("text").text();
        assert_eq!(text.chars().count(), 4, "{text}");
        assert!(text.contains('X') && text.contains('Y'), "both insertions survive: {text}");
        assert_eq!(hub.version(), 3);
    }

    #[test]
    fn a_shape_insert_converges_with_the_same_id_and_key() {
        let mut hub = hub();
        let welcome_a = join(&mut hub, "alice", Role::Editor);
        let welcome_b = join(&mut hub, "bob", Role::Editor);
        let mut sa = deck_session();
        let mut sb = deck_session();
        let mut a = CollabDoc::new("d");
        let mut b = CollabDoc::new("d");
        let _ = a.connect(&sa, "ja", ClientInfo { name: "a".into(), ver: "1".into() });
        let _ = b.connect(&sb, "jb", ClientInfo { name: "b".into(), ver: "1".into() });
        a.on_msg(&mut sa, ServerMsg::Welcome(welcome_a));
        b.on_msg(&mut sb, ServerMsg::Welcome(welcome_b));
        a.submit(&mut sa, "shape.insert", &json!({"preset": "rect", "rect": [50, 50, 40, 30], "text": "hi"}), "ins-1").expect("submit");
        while let Some(msg) = a.next_outgoing() {
            if let ClientMsg::Op(op) = msg {
                hub.receive("alice", ClientMsg::Op(op)).expect("receive");
            }
        }
        a.on_msg(&mut sa, hub.poll("alice").expect("a"));
        b.on_msg(&mut sb, hub.poll("bob").expect("b"));
        let count = |s: &Session| s.doc().expect("doc").doc.slides[0].shapes.len();
        assert_eq!(count(&sa), 3);
        assert_eq!(count(&sb), 3);
        let ids = |s: &Session| s.doc().expect("doc").doc.slides[0].shapes.iter().map(|x| x.id).collect::<Vec<_>>();
        assert_eq!(ids(&sa), ids(&sb), "the same deterministic id on both replicas");
    }

    #[test]
    fn two_clients_via_the_hub_converge_without_sockets() {
        let mut hub = hub();
        let welcome_a = join(&mut hub, "alice", Role::Editor);
        let welcome_b = join(&mut hub, "bob", Role::Editor);
        let mut sa = deck_session();
        let mut sb = deck_session();
        let mut a = CollabDoc::new("d");
        let mut b = CollabDoc::new("d");
        let _ = a.connect(&sa, "ja", ClientInfo { name: "a".into(), ver: "1".into() });
        let _ = b.connect(&sb, "jb", ClientInfo { name: "b".into(), ver: "1".into() });
        a.on_msg(&mut sa, ServerMsg::Welcome(welcome_a));
        b.on_msg(&mut sb, ServerMsg::Welcome(welcome_b));

        let shape = sa.doc().expect("doc").doc.slides[0].shapes[0].id.0;
        a.submit(&mut sa, "shape.move", &json!({"ids": [shape], "x": 250.0, "y": 100.0}), "a-1").expect("submit");
        while let Some(msg) = a.next_outgoing() {
            if let ClientMsg::Op(op) = msg {
                hub.receive("alice", ClientMsg::Op(op)).expect("receive");
            }
        }
        a.on_msg(&mut sa, hub.poll("alice").expect("a"));
        b.on_msg(&mut sb, hub.poll("bob").expect("b"));
        let x_of = |s: &Session| s.doc().expect("doc").doc.slides[0].shapes[0].xfrm.expect("xfrm").x;
        assert_eq!(x_of(&sa), 250.0);
        assert_eq!(x_of(&sb), 250.0);
        assert_eq!(a.version(), 1);
        assert_eq!(b.version(), 1);

        // A structural operation is not applied locally until the server answers.
        let before = sa.doc().expect("doc").doc.slides.len();
        a.submit(&mut sa, "slide.new", &json!({"layout": "blank", "at": 1}), "a-2").expect("submit slide");
        assert_eq!(sa.doc().expect("doc").doc.slides.len(), before, "nothing applied before applied");
        while let Some(msg) = a.next_outgoing() {
            if let ClientMsg::Op(op) = msg {
                hub.receive("alice", ClientMsg::Op(op)).expect("receive");
            }
        }
        a.on_msg(&mut sa, hub.poll("alice").expect("a"));
        b.on_msg(&mut sb, hub.poll("bob").expect("b"));
        assert_eq!(sa.doc().expect("doc").doc.slides.len(), before + 1);
        assert_eq!(sb.doc().expect("doc").doc.slides.len(), before + 1);
        assert_eq!(hub.session().document().expect("doc").slides.len(), before + 1);
    }

    /// Drain one client's outgoing messages into the room.
    fn send_all(hub: &mut Hub, doc: &mut CollabDoc, actor: &str) {
        while let Some(msg) = doc.next_outgoing() {
            match msg {
                ClientMsg::Op(op) => hub.receive(actor, ClientMsg::Op(op)).expect("op"),
                ClientMsg::Redo => hub.receive(actor, ClientMsg::Redo).expect("redo"),
                ClientMsg::Undo => hub.receive(actor, ClientMsg::Undo).expect("undo"),
                ClientMsg::Presence(p) => hub.receive(actor, ClientMsg::Presence(p)).expect("presence"),
                ClientMsg::Query { id, cmd, target, params } => hub.receive(actor, ClientMsg::Query { id, cmd, target, params }).expect("query"),
                _ => {}
            }
        }
    }

    /// Deliver everything the room answered to one client.
    fn deliver(hub: &mut Hub, doc: &mut CollabDoc, session: &mut Session, actor: &str) {
        while let Some(msg) = hub.poll(actor) {
            doc.on_msg(session, msg);
        }
    }

    #[test]
    fn two_animation_steps_in_a_row_and_an_undo_reach_both_replicas_through_the_hub() {
        let mut hub = hub();
        let welcome_a = join(&mut hub, "alice", Role::Editor);
        let welcome_b = join(&mut hub, "bob", Role::Editor);
        let mut sa = deck_session();
        let mut sb = deck_session();
        let mut a = CollabDoc::new("d");
        let mut b = CollabDoc::new("d");
        let _ = a.connect(&sa, "ja", ClientInfo { name: "a".into(), ver: "1".into() });
        let _ = b.connect(&sb, "jb", ClientInfo { name: "b".into(), ver: "1".into() });
        a.on_msg(&mut sa, ServerMsg::Welcome(welcome_a));
        b.on_msg(&mut sb, ServerMsg::Welcome(welcome_b));
        let shape = sa.doc().expect("doc").doc.slides[0].shapes[0].id.0;
        let effects = |s: &Session| s.doc().expect("doc").doc.slides[0].animations.iter().map(|x| x.effect.clone()).collect::<Vec<_>>();

        // Two steps in a row: the second normalises the list the first left on the replica.
        for (n, (cmd, params)) in [
            ("animation.add", json!({"effect": "fade", "ids": [shape]})),
            ("animation.add", json!({"effect": "pulse", "class": "emphasis", "ids": [shape]})),
        ]
        .into_iter()
        .enumerate()
        {
            a.submit(&mut sa, cmd, &params, &format!("a-{n}")).expect("submit");
            send_all(&mut hub, &mut a, "alice");
            deliver(&mut hub, &mut a, &mut sa, "alice");
            deliver(&mut hub, &mut b, &mut sb, "bob");
        }
        assert_eq!(a.version(), 2);
        assert_eq!(b.version(), 2);
        assert_eq!(effects(&sa), ["fade", "pulse"], "the two steps keep their order");
        assert_eq!(effects(&sb), ["fade", "pulse"]);
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc, "both replicas hold the same presentation");

        // Undo takes back the second step and keeps the first, on both replicas.
        a.undo().expect("request an undo");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!(a.version(), 3);
        assert_eq!(b.version(), 3);
        assert_eq!(effects(&sa), ["fade"], "the undo put the shape's step list back");
        assert_eq!(effects(&sb), ["fade"]);
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);
    }

    #[test]
    fn animation_set_timing_and_move_reach_both_replicas_and_each_undo_is_one_step() {
        let mut hub = hub();
        let welcome_a = join(&mut hub, "alice", Role::Editor);
        let welcome_b = join(&mut hub, "bob", Role::Editor);
        let mut sa = deck_session();
        let mut sb = deck_session();
        let mut a = CollabDoc::new("d");
        let mut b = CollabDoc::new("d");
        let _ = a.connect(&sa, "ja", ClientInfo { name: "a".into(), ver: "1".into() });
        let _ = b.connect(&sb, "jb", ClientInfo { name: "b".into(), ver: "1".into() });
        a.on_msg(&mut sa, ServerMsg::Welcome(welcome_a));
        b.on_msg(&mut sb, ServerMsg::Welcome(welcome_b));
        let shapes: Vec<u32> = sa.doc().expect("doc").doc.slides[0].shapes.iter().map(|s| s.id.0).collect();
        let (first, second) = (shapes[0], shapes[1]);
        let sequence = |s: &Session| {
            s.doc().expect("doc").doc.slides[0].animations.iter().map(|x| (x.shape.0, x.effect.clone(), x.duration_ms)).collect::<Vec<_>>()
        };
        #[allow(clippy::too_many_arguments)]
        fn step(
            hub: &mut Hub,
            author: &str,
            doc: &mut CollabDoc,
            session: &mut Session,
            peer_actor: &str,
            peer: &mut CollabDoc,
            peer_session: &mut Session,
            cmd: &str,
            params: &serde_json::Value,
            id: &str,
        ) {
            doc.submit(session, cmd, params, id).expect("submit");
            send_all(hub, doc, author);
            deliver(hub, doc, session, author);
            deliver(hub, peer, peer_session, peer_actor);
        }

        step(&mut hub, "alice", &mut a, &mut sa, "bob", &mut b, &mut sb, "animation.set", &json!({"effect": "fade", "ids": [first]}), "a-0");
        assert_eq!(sequence(&sa), [(first, "fade".into(), 500)]);
        assert_eq!(sequence(&sb), sequence(&sa), "a remote set arrives on the second replica");

        step(&mut hub, "alice", &mut a, &mut sa, "bob", &mut b, &mut sb, "animation.set", &json!({"effect": "zoom", "ids": [second]}), "a-1");
        assert_eq!(sequence(&sa).len(), 2);
        assert_eq!(sequence(&sb), sequence(&sa));

        // Undo of the second set takes back only that shape's step; the first stays.
        a.undo().expect("undo the set");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!(sequence(&sa), [(first, "fade".into(), 500)], "the undo restored the whole sequence");
        assert_eq!(sequence(&sb), sequence(&sa));

        // Timing changes one step and its undo restores the exact duration.
        step(&mut hub, "alice", &mut a, &mut sa, "bob", &mut b, &mut sb, "animation.timing", &json!({"index": 0, "duration": 1_200}), "a-2");
        assert_eq!(sequence(&sa)[0].2, 1_200);
        assert_eq!(sequence(&sb), sequence(&sa));
        a.undo().expect("undo the timing");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!(sequence(&sa)[0].2, 500);
        assert_eq!(sequence(&sb), sequence(&sa));

        // Reorder across shapes: a's step moves past b's step, and the undo puts the order back.
        step(&mut hub, "alice", &mut a, &mut sa, "bob", &mut b, &mut sb, "animation.set", &json!({"effect": "zoom", "ids": [second]}), "a-3");
        assert_eq!(sequence(&sa).iter().map(|x| x.0).collect::<Vec<_>>(), [first, second]);
        step(&mut hub, "alice", &mut a, &mut sa, "bob", &mut b, &mut sb, "animation.move", &json!({"index": 0, "to": 1}), "a-4");
        assert_eq!(sequence(&sa).iter().map(|x| x.0).collect::<Vec<_>>(), [second, first], "the step crossed the other shape's step");
        assert_eq!(sequence(&sb), sequence(&sa));
        a.undo().expect("undo the move");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!(sequence(&sa).iter().map(|x| x.0).collect::<Vec<_>>(), [first, second], "the undo restored the whole order");
        assert_eq!(sequence(&sb), sequence(&sa));
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);

        // A foreign animation edit after the step makes the undo a conflict instead of an overwrite.
        step(&mut hub, "alice", &mut a, &mut sa, "bob", &mut b, &mut sb, "animation.timing", &json!({"index": 0, "delay": 250}), "a-5");
        b.submit(&mut sb, "animation.set", &json!({"effect": "pulse", "class": "emphasis", "ids": [second]}), "b-6").expect("bob submits");
        send_all(&mut hub, &mut b, "bob");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        let head = hub.session().version();
        a.undo().expect("request a conflicting undo");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!(hub.session().version(), head, "a refused undo writes nothing");
        assert_eq!(sequence(&sb), sequence(&sa), "the foreign edit survives");
    }

    #[test]
    fn animation_options_clear_and_the_painter_reach_both_replicas_and_each_undo_is_one_step() {
        let mut hub = hub();
        let welcome_a = join(&mut hub, "alice", Role::Editor);
        let welcome_b = join(&mut hub, "bob", Role::Editor);
        let mut sa = deck_session();
        let mut sb = deck_session();
        let mut a = CollabDoc::new("d");
        let mut b = CollabDoc::new("d");
        let _ = a.connect(&sa, "ja", ClientInfo { name: "a".into(), ver: "1".into() });
        let _ = b.connect(&sb, "jb", ClientInfo { name: "b".into(), ver: "1".into() });
        a.on_msg(&mut sa, ServerMsg::Welcome(welcome_a));
        b.on_msg(&mut sb, ServerMsg::Welcome(welcome_b));
        let shapes: Vec<u32> = sa.doc().expect("doc").doc.slides[0].shapes.iter().map(|s| s.id.0).collect();
        let (first, second) = (shapes[0], shapes[1]);
        let sequence = |s: &Session| {
            s.doc().expect("doc").doc.slides[0]
                .animations
                .iter()
                .map(|x| (x.shape.0, x.effect.clone(), x.option.clone(), x.text_build))
                .collect::<Vec<_>>()
        };
        let effects = |s: &Session| sequence(s).into_iter().map(|x| x.1).collect::<Vec<_>>();
        #[allow(clippy::too_many_arguments)]
        fn step(
            hub: &mut Hub,
            author: &str,
            doc: &mut CollabDoc,
            session: &mut Session,
            peer_actor: &str,
            peer: &mut CollabDoc,
            peer_session: &mut Session,
            cmd: &str,
            params: &serde_json::Value,
            id: &str,
        ) {
            doc.submit(session, cmd, params, id).expect("submit");
            send_all(hub, doc, author);
            deliver(hub, doc, session, author);
            deliver(hub, peer, peer_session, peer_actor);
        }

        step(&mut hub, "alice", &mut a, &mut sa, "bob", &mut b, &mut sb, "animation.add", &json!({"effect": "fade", "ids": [first]}), "a-0");
        step(&mut hub, "alice", &mut a, &mut sa, "bob", &mut b, &mut sb, "animation.add", &json!({"effect": "zoom", "ids": [second]}), "a-1");
        assert_eq!(effects(&sb), ["fade", "zoom"]);

        // Effect options: one step's text build changes, the other step keeps its own.
        step(
            &mut hub,
            "alice",
            &mut a,
            &mut sa,
            "bob",
            &mut b,
            &mut sb,
            "animation.options",
            &json!({"index": 0, "textBuild": "byParagraph"}),
            "a-2",
        );
        assert_eq!(sequence(&sa)[0].3, deckcraft_model::anim::TextBuild::ByParagraph);
        assert_eq!(sequence(&sa)[1].3, deckcraft_model::anim::TextBuild::AsOne);
        assert_eq!(sequence(&sb), sequence(&sa), "a remote options edit arrives on the second replica");
        a.undo().expect("undo the options");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!(sequence(&sa)[0].3, deckcraft_model::anim::TextBuild::AsOne, "the undo restored the previous step");
        assert_eq!(sequence(&sb), sequence(&sa));

        // The painter: the clipboard is a session value, the wire value is the resulting sequence.
        sa.execute("animation.painter", &json!({"id": first})).expect("pick");
        step(&mut hub, "alice", &mut a, &mut sa, "bob", &mut b, &mut sb, "animation.painterApply", &json!({"ids": [second]}), "a-3");
        assert_eq!(effects(&sa), ["fade", "fade"], "the target's own step was replaced by the picked one");
        assert_eq!(sequence(&sb), sequence(&sa), "both replicas hold the painted sequence");
        a.undo().expect("undo the paint");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!(effects(&sa), ["fade", "zoom"], "the undo restored the whole sequence");
        assert_eq!(sequence(&sb), sequence(&sa));

        // `animation.clear` empties the slide's whole sequence; the undo puts it back.
        step(&mut hub, "alice", &mut a, &mut sa, "bob", &mut b, &mut sb, "animation.clear", &json!({}), "a-4");
        assert!(effects(&sa).is_empty());
        assert!(effects(&sb).is_empty());
        a.undo().expect("undo the clear");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!(effects(&sa), ["fade", "zoom"], "the undo restored the cleared sequence");
        assert_eq!(sequence(&sb), sequence(&sa));
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);
    }

    #[test]
    fn a_document_font_reaches_both_replicas_and_its_undo_comes_back_through_the_hub() {
        let mut hub = hub();
        let welcome_a = join(&mut hub, "alice", Role::Editor);
        let welcome_b = join(&mut hub, "bob", Role::Editor);
        let mut sa = deck_session();
        let mut sb = deck_session();
        let mut a = CollabDoc::new("d");
        let mut b = CollabDoc::new("d");
        let _ = a.connect(&sa, "ja", ClientInfo { name: "a".into(), ver: "1".into() });
        let _ = b.connect(&sb, "jb", ClientInfo { name: "b".into(), ver: "1".into() });
        a.on_msg(&mut sa, ServerMsg::Welcome(welcome_a));
        b.on_msg(&mut sb, ServerMsg::Welcome(welcome_b));

        let bytes = include_bytes!("../../render/tests/fixtures/colr-squares.ttf").to_vec();
        let sha = crate::media::sha256_hex(&bytes);
        let served = bytes.clone();
        // The second replica has no bytes: it fetches them from the room before applying.
        b.set_media_fetcher(std::sync::Arc::new(move |asked: &str| (asked == sha).then(|| served.clone())));
        let entry = |s: &Session| s.doc().expect("doc").doc.embedded_fonts.first().cloned();

        // Alice embeds a face: the bytes go to the media channel, the operation names the digest.
        let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
        a.submit(&mut sa, "font.create", &json!({"typeface": "Deck Test Face", "data": data}), "f-1").expect("submit font");
        for (digest, upload) in a.take_uploads() {
            assert_eq!(hub.put_media("DeckFace.ttf", upload.as_ref().clone()).expect("upload"), digest);
        }
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        let first = entry(&sa).expect("alice embeds the font");
        assert_eq!(first.typeface, "Deck Test Face");
        assert!(first.regular.is_some());
        assert_eq!(entry(&sb).expect("bob applies the same entry"), first);
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);
        assert_eq!((a.version(), b.version()), (1, 1));

        // A descriptor step is the same whole value on both replicas.
        a.submit(&mut sa, "font.set", &json!({"typeface": "Deck Test Face", "panose": "020B0604020202020204", "charset": 204}), "f-2")
            .expect("submit the descriptors");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!(entry(&sa).and_then(|e| e.panose).as_deref(), Some("020B0604020202020204"));
        assert_eq!(entry(&sb).and_then(|e| e.panose).as_deref(), Some("020B0604020202020204"));
        assert_eq!((a.version(), b.version()), (2, 2));

        // The undo of the descriptor step is broadcast like any other operation and lands on
        // both replicas: the face stays, the descriptors go back to what they were.
        a.undo().expect("request an undo");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (3, 3));
        assert_eq!(entry(&sa).and_then(|e| e.panose), None);
        assert_eq!(entry(&sb).and_then(|e| e.panose), None);
        assert!(entry(&sa).is_some_and(|e| e.regular.is_some()), "the face is untouched");
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);

        // A second undo takes back the create itself: the entry leaves both documents and the
        // face object stays, so the redo can put the entry back.
        a.undo().expect("undo the create");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (4, 4));
        assert!(entry(&sa).is_none());
        assert!(entry(&sb).is_none());
        assert!(!sa.doc().expect("doc").doc.media.is_empty(), "the face object stays on the replica");
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);

        // Redo puts the entry back on both.
        a.redo().expect("redo the create");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (5, 5));
        assert!(entry(&sa).is_some_and(|e| e.regular.is_some()));
        assert_eq!(entry(&sb), entry(&sa));
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);
    }

    /// One slide with a 2×2 table and its top-left cell seeded.
    fn table_session() -> Session {
        let mut s = Session::with_new();
        let title = s.doc().expect("doc").doc.slides[0].id;
        s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("blank");
        s.apply_remote("slide.delete", &json!({"ids": [title.0]})).expect("drop title");
        s.apply_remote("insert.table", &json!({"rows": 2, "cols": 2, "rect": [100, 100, 300, 100]})).expect("table");
        let table = s.doc().expect("doc").doc.slides[0].shapes[0].id;
        s.apply_remote("text.set", &json!({"id": table.0, "cell": [0, 0], "text": "seed"})).expect("seed");
        s
    }

    fn table_hub() -> Hub {
        let doc = table_session().doc().expect("doc").doc.as_ref().clone();
        Hub::new(DocSession::from_document("d", doc, Default::default()).expect("session"))
    }

    fn table_of(session: &Session) -> &deckcraft_model::Table {
        match &session.doc().expect("doc").doc.slides[0].shapes[0].kind {
            deckcraft_model::ShapeKind::Table(t) => t,
            other => panic!("not a table: {other:?}"),
        }
    }

    #[test]
    fn the_rest_of_the_table_surface_reaches_both_replicas_and_an_undo_is_one_step() {
        let mut hub = table_hub();
        let welcome_a = join(&mut hub, "alice", Role::Editor);
        let welcome_b = join(&mut hub, "bob", Role::Editor);
        let mut sa = table_session();
        let mut sb = table_session();
        let mut a = CollabDoc::new("d");
        let mut b = CollabDoc::new("d");
        let _ = a.connect(&sa, "ja", ClientInfo { name: "a".into(), ver: "1".into() });
        let _ = b.connect(&sb, "jb", ClientInfo { name: "b".into(), ver: "1".into() });
        a.on_msg(&mut sa, ServerMsg::Welcome(welcome_a));
        b.on_msg(&mut sb, ServerMsg::Welcome(welcome_b));
        let table = sa.doc().expect("doc").doc.slides[0].shapes[0].id.0;

        let steps: [(&str, serde_json::Value); 3] = [
            ("table.style", json!({"id": table, "style": "grid"})),
            ("table.cellFill", json!({"id": table, "color": "#00ff00", "cells": [[0, 1]]})),
            ("table.rowHeight", json!({"id": table, "row": 0, "height": 30})),
        ];
        for (n, (cmd, params)) in steps.iter().enumerate() {
            let id = format!("a-{n}");
            a.submit(&mut sa, cmd, params, &id).expect("submit");
            send_all(&mut hub, &mut a, "alice");
            deliver(&mut hub, &mut a, &mut sa, "alice");
            deliver(&mut hub, &mut b, &mut sb, "bob");
        }
        assert_eq!((a.version(), b.version()), (3, 3));
        assert_eq!(table_of(&sa).style, "grid");
        assert!(table_of(&sa).cell(0, 1).expect("cell").fill.is_some());
        assert_eq!(table_of(&sa).rows[0].height, 30.0);
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc, "both replicas hold the same table");

        // The undo of the height step puts the whole table back to the state the step found: the
        // style and the fill stay, and both replicas apply the same confirmed command.
        a.undo().expect("request an undo");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (4, 4));
        assert_ne!(table_of(&sa).rows[0].height, 30.0);
        assert_eq!(table_of(&sa).style, "grid");
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);

        // A second undo takes the fill back but keeps the style.
        a.undo().expect("undo the fill");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (5, 5));
        assert!(table_of(&sa).cell(0, 1).expect("cell").fill.is_none());
        assert_eq!(table_of(&sa).style, "grid");
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);
    }

    #[test]
    fn table_options_reach_both_replicas_and_an_undo_is_one_step() {
        let mut hub = table_hub();
        let welcome_a = join(&mut hub, "alice", Role::Editor);
        let welcome_b = join(&mut hub, "bob", Role::Editor);
        let mut sa = table_session();
        let mut sb = table_session();
        let mut a = CollabDoc::new("d");
        let mut b = CollabDoc::new("d");
        let _ = a.connect(&sa, "ja", ClientInfo { name: "a".into(), ver: "1".into() });
        let _ = b.connect(&sb, "jb", ClientInfo { name: "b".into(), ver: "1".into() });
        a.on_msg(&mut sa, ServerMsg::Welcome(welcome_a));
        b.on_msg(&mut sb, ServerMsg::Welcome(welcome_b));
        let table = sa.doc().expect("doc").doc.slides[0].shapes[0].id.0;
        let before = table_of(&sa).clone();

        // One style-option change is one whole-table final value, so both replicas end with the
        // same six flags even though only two were named.
        a.submit(&mut sa, "table.options", &json!({"id": table, "bandedColumns": true, "totalRow": true}), "a-1").expect("submit");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (1, 1));
        assert!(table_of(&sa).band_col && table_of(&sa).last_row);
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc, "both replicas hold the same table");

        // The undo of the step puts the previous six values back on both replicas.
        a.undo().expect("request an undo");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (2, 2));
        assert_eq!(table_of(&sa), &before, "the whole table came back");
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);
    }

    /// One slide with a SmartArt graphic (Plan, Scope under it, Build).
    fn smartart_session() -> Session {
        let mut s = Session::with_new();
        let title = s.doc().expect("doc").doc.slides[0].id;
        s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("blank");
        s.apply_remote("slide.delete", &json!({"ids": [title.0]})).expect("drop title");
        s.apply_remote("insert.smartArt", &json!({"kind": "blockList", "items": ["Plan", "\tScope", "Build"]})).expect("smartart");
        s
    }

    fn smartart_hub() -> Hub {
        let doc = smartart_session().doc().expect("doc").doc.as_ref().clone();
        Hub::new(DocSession::from_document("d", doc, Default::default()).expect("session"))
    }

    /// One slide with a text box of two paragraphs (the selection is on it).
    fn text_session() -> Session {
        let mut s = Session::with_new();
        let title = s.doc().expect("doc").doc.slides[0].id;
        s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("blank");
        s.apply_remote("slide.delete", &json!({"ids": [title.0]})).expect("drop title");
        s.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [10, 20, 120, 60], "text": "Alpha\nBeta"})).expect("text");
        s
    }

    fn text_hub() -> Hub {
        let doc = text_session().doc().expect("doc").doc.as_ref().clone();
        Hub::new(DocSession::from_document("d", doc, Default::default()).expect("session"))
    }

    #[test]
    fn a_viewer_cannot_convert_a_smartart_and_the_document_does_not_move() {
        let mut hub = smartart_hub();
        let _ = join(&mut hub, "alice", Role::Editor);
        let welcome_v = join(&mut hub, "victor", Role::Viewer);
        let mut sv = smartart_session();
        let mut v = CollabDoc::new("d");
        let _ = v.connect(&sv, "jv", ClientInfo { name: "v".into(), ver: "1".into() });
        v.on_msg(&mut sv, ServerMsg::Welcome(welcome_v));
        let group = sv.doc().expect("doc").doc.slides[0].shapes[0].id;
        assert_eq!(v.submit(&mut sv, "smartArt.toShapes", &json!({"id": group.0}), "v-1"), Err(crate::LocalError::ReadOnly));

        // Even a hand-built structural conversion never reaches the engine, and the version and
        // the document stay put.
        let (slide, shape) = keys_of(&hub);
        let before = hub.session().document().expect("doc").clone();
        let step = OpEnvelope {
            op_id: "v-2".into(),
            doc: "d".into(),
            base_version: 0,
            kind: OpKind::Structural,
            target: Target { slide: Some(slide), shape: Some(shape), ..Target::default() },
            payload: Payload::SmartArtToShapes {},
            history: None,
            preview: None,
        };
        hub.receive("victor", ClientMsg::Op(step)).expect("receive");
        match hub.poll("victor").expect("answer") {
            ServerMsg::Rejected(r) => assert_eq!(r.code, RejectCode::Forbidden),
            other => panic!("{other:?}"),
        }
        assert_eq!(hub.version(), 0);
        assert_eq!(hub.session().document().expect("doc"), &before, "the document did not move");
    }

    /// 8×8 red PNG, the media channel's picture bytes.
    fn png() -> Vec<u8> {
        vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0, 8, 8, 2, 0, 0, 0, 75, 109, 41, 220, 0, 0, 0, 17, 73,
            68, 65, 84, 120, 156, 99, 184, 163, 161, 129, 21, 49, 12, 45, 9, 0, 123, 123, 75, 1, 251, 162, 81, 240, 0, 0, 0, 0, 73, 69, 78, 68, 174,
            66, 96, 130,
        ]
    }

    #[test]
    fn a_node_picture_reaches_both_replicas_through_the_hub_and_a_viewer_is_refused() {
        let mut hub = smartart_hub();
        let bytes = png();
        let sha = hub.put_media("red.png", bytes.clone()).expect("upload");
        let welcome_a = join(&mut hub, "alice", Role::Editor);
        let welcome_b = join(&mut hub, "bob", Role::Editor);
        let _ = join(&mut hub, "victor", Role::Viewer);
        let (mut sa, mut sb) = (smartart_session(), smartart_session());
        let (mut a, mut b) = (CollabDoc::new("d"), CollabDoc::new("d"));
        let _ = a.connect(&sa, "ja", ClientInfo { name: "a".into(), ver: "1".into() });
        let _ = b.connect(&sb, "jb", ClientInfo { name: "b".into(), ver: "1".into() });
        a.on_msg(&mut sa, ServerMsg::Welcome(welcome_a));
        b.on_msg(&mut sb, ServerMsg::Welcome(welcome_b));
        a.put_media("red.png", bytes.clone()).expect("a has the bytes");
        b.put_media("red.png", bytes).expect("b has the bytes");
        let group = sa.doc().expect("doc").doc.slides[0].shapes[0].id;
        let before = sa.doc().expect("doc").doc.slides[0].shapes[0].clone();
        let params = json!({"id": group.0, "index": 0, "sha256": sha, "name": "red.png", "place": {"crop": [0.1, 0.0, 0.2, 0.0]}});
        a.submit(&mut sa, "smartArt.picture", &params, "a-1").expect("submit the picture");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (1, 1));
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc, "both replicas hold the same node picture");
        let after = sa.doc().expect("doc").doc.slides[0].shapes[0].clone();
        let d = after.diagram.as_ref().expect("diagram");
        assert!(d.nodes[0].picture.is_some());
        assert_eq!(d.nodes[0].picture_place.as_ref().map(|p| p.crop), Some([0.1, 0.0, 0.2, 0.0]));

        // A viewer's hand-built step never reaches the engine: version and document stay.
        let (slide, shape) = keys_of(&hub);
        let viewer_step = OpEnvelope {
            op_id: "v-1".into(),
            doc: "d".into(),
            base_version: 1,
            kind: OpKind::Command,
            target: Target { slide: Some(slide), shape: Some(shape), ..Target::default() },
            payload: Payload::SmartArtPicture { node: 0, media: None },
            history: None,
            preview: None,
        };
        // The picture's broadcast is still in the viewer's outbox; drain it first.
        while hub.poll("victor").is_some() {}
        hub.receive("victor", ClientMsg::Op(viewer_step)).expect("receive");
        match hub.poll("victor").expect("answer") {
            ServerMsg::Rejected(r) => assert_eq!(r.code, RejectCode::Forbidden),
            other => panic!("{other:?}"),
        }
        assert_eq!(hub.version(), 1);
        assert_eq!(hub.session().document().expect("doc").slides[0].shapes[0], after);

        // The undo image is the whole group: the node comes back without its picture, and the
        // redo returns the exact result.
        a.undo().expect("request an undo");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (2, 2));
        assert_eq!(sa.doc().expect("doc").doc.slides[0].shapes[0], before);
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);
        a.redo().expect("request a redo");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (3, 3));
        assert_eq!(sa.doc().expect("doc").doc.slides[0].shapes[0], after);
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);
    }

    #[test]
    fn smartart_conversions_reach_both_replicas_and_an_undo_restores_their_source() {
        // To Text: one whole body, the same text box on both replicas, and the graphic back on undo.
        let mut hub = smartart_hub();
        let welcome_a = join(&mut hub, "alice", Role::Editor);
        let welcome_b = join(&mut hub, "bob", Role::Editor);
        let (mut sa, mut sb) = (smartart_session(), smartart_session());
        let (mut a, mut b) = (CollabDoc::new("d"), CollabDoc::new("d"));
        let _ = a.connect(&sa, "ja", ClientInfo { name: "a".into(), ver: "1".into() });
        let _ = b.connect(&sb, "jb", ClientInfo { name: "b".into(), ver: "1".into() });
        a.on_msg(&mut sa, ServerMsg::Welcome(welcome_a));
        b.on_msg(&mut sb, ServerMsg::Welcome(welcome_b));
        let group = sa.doc().expect("doc").doc.slides[0].shapes[0].id;
        let before = sa.doc().expect("doc").doc.slides[0].shapes[0].clone();
        a.submit(&mut sa, "smartArt.toText", &json!({"id": group.0}), "a-1").expect("submit toText");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (1, 1));
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc, "both replicas hold the same text");
        let after = sa.doc().expect("doc").doc.slides[0].shapes[0].clone();
        assert!(after.text_box && after.diagram.is_none(), "the graphic became a text box");
        assert_eq!(after.text.as_ref().expect("body").paragraphs.iter().map(|p| p.text()).collect::<Vec<_>>(), ["Plan", "Scope", "Build"]);
        a.undo().expect("request an undo");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (2, 2));
        assert_eq!(sa.doc().expect("doc").doc.slides[0].shapes[0], before, "the whole graphic came back");
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);
        a.redo().expect("request a redo");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (3, 3));
        assert_eq!(sa.doc().expect("doc").doc.slides[0].shapes[0], after);
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);

        // To Shapes: the drawing stays, the outline is dropped, and the undo puts the diagram back.
        let mut hub = smartart_hub();
        let welcome_a = join(&mut hub, "alice", Role::Editor);
        let welcome_b = join(&mut hub, "bob", Role::Editor);
        let (mut sa, mut sb) = (smartart_session(), smartart_session());
        let (mut a, mut b) = (CollabDoc::new("d"), CollabDoc::new("d"));
        let _ = a.connect(&sa, "ja", ClientInfo { name: "a".into(), ver: "1".into() });
        let _ = b.connect(&sb, "jb", ClientInfo { name: "b".into(), ver: "1".into() });
        a.on_msg(&mut sa, ServerMsg::Welcome(welcome_a));
        b.on_msg(&mut sb, ServerMsg::Welcome(welcome_b));
        let group = sa.doc().expect("doc").doc.slides[0].shapes[0].id;
        let before = sa.doc().expect("doc").doc.slides[0].shapes[0].clone();
        a.submit(&mut sa, "smartArt.toShapes", &json!({"id": group.0}), "a-1").expect("submit toShapes");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (1, 1));
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);
        let after = sa.doc().expect("doc").doc.slides[0].shapes[0].clone();
        assert!(after.diagram.is_none() && after.name == "Group");
        a.undo().expect("request an undo");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (2, 2));
        assert_eq!(sa.doc().expect("doc").doc.slides[0].shapes[0], before);
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);
        a.redo().expect("request a redo");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!(sa.doc().expect("doc").doc.slides[0].shapes[0], after);

        // From Text: the selection's paragraphs become the graphic, and the undo restores the box.
        let mut hub = text_hub();
        let welcome_a = join(&mut hub, "alice", Role::Editor);
        let welcome_b = join(&mut hub, "bob", Role::Editor);
        let (mut sa, mut sb) = (text_session(), text_session());
        let (mut a, mut b) = (CollabDoc::new("d"), CollabDoc::new("d"));
        let _ = a.connect(&sa, "ja", ClientInfo { name: "a".into(), ver: "1".into() });
        let _ = b.connect(&sb, "jb", ClientInfo { name: "b".into(), ver: "1".into() });
        a.on_msg(&mut sa, ServerMsg::Welcome(welcome_a));
        b.on_msg(&mut sb, ServerMsg::Welcome(welcome_b));
        let before = sa.doc().expect("doc").doc.slides[0].shapes[0].clone();
        a.submit(&mut sa, "smartArt.fromText", &json!({"kind": "process"}), "a-1").expect("submit fromText");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (1, 1));
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc, "both replicas hold the same graphic");
        let after = sa.doc().expect("doc").doc.slides[0].shapes[0].clone();
        let d = after.diagram.as_ref().expect("a graphic");
        assert_eq!(d.kind, "process");
        assert_eq!(d.nodes.iter().map(|n| n.text.as_str()).collect::<Vec<_>>(), ["Alpha", "Beta"]);
        a.undo().expect("request an undo");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (2, 2));
        assert_eq!(sa.doc().expect("doc").doc.slides[0].shapes[0], before, "the text box came back whole");
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);
        a.redo().expect("request a redo");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!(sa.doc().expect("doc").doc.slides[0].shapes[0], after);
    }

    #[test]
    fn smartart_edits_reach_both_replicas_and_an_undo_is_one_step() {
        let mut hub = smartart_hub();
        let welcome_a = join(&mut hub, "alice", Role::Editor);
        let welcome_b = join(&mut hub, "bob", Role::Editor);
        let mut sa = smartart_session();
        let mut sb = smartart_session();
        let mut a = CollabDoc::new("d");
        let mut b = CollabDoc::new("d");
        let _ = a.connect(&sa, "ja", ClientInfo { name: "a".into(), ver: "1".into() });
        let _ = b.connect(&sb, "jb", ClientInfo { name: "b".into(), ver: "1".into() });
        a.on_msg(&mut sa, ServerMsg::Welcome(welcome_a));
        b.on_msg(&mut sb, ServerMsg::Welcome(welcome_b));
        let group = sa.doc().expect("doc").doc.slides[0].shapes[0].id.0;
        let before = sa.doc().expect("doc").doc.slides[0].shapes[0].clone();

        // One command becomes one whole diagram value; both replicas end with the same graphic.
        a.submit(&mut sa, "smartArt.layout", &json!({"id": group, "kind": "hierarchy"}), "a-1").expect("submit");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        a.submit(&mut sa, "smartArt.style", &json!({"id": group, "style": "intense"}), "a-2").expect("submit the style");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (2, 2));
        assert_eq!(sa.doc().expect("doc").doc.slides[0].shapes[0].diagram.as_ref().expect("diagram").kind, "hierarchy");
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc, "both replicas hold the same graphic");

        // Each edit is its own step; two undos put the whole group back on both replicas.
        for version in [3, 4] {
            a.undo().expect("request an undo");
            send_all(&mut hub, &mut a, "alice");
            deliver(&mut hub, &mut a, &mut sa, "alice");
            deliver(&mut hub, &mut b, &mut sb, "bob");
            assert_eq!((a.version(), b.version()), (version, version));
        }
        assert_eq!(sa.doc().expect("doc").doc.slides[0].shapes[0], before, "the whole graphic came back");
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);
    }

    /// One slide with a chart (column, two categories, one series).
    fn chart_session() -> Session {
        let mut s = Session::with_new();
        let title = s.doc().expect("doc").doc.slides[0].id;
        s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("blank");
        s.apply_remote("slide.delete", &json!({"ids": [title.0]})).expect("drop title");
        s.apply_remote("insert.chart", &json!({"type": "column", "categories": ["Q1", "Q2"], "series": [{"name": "Sales", "values": [4.0, 5.0]}]}))
            .expect("chart");
        s
    }

    fn chart_hub() -> Hub {
        let doc = chart_session().doc().expect("doc").doc.as_ref().clone();
        Hub::new(DocSession::from_document("d", doc, Default::default()).expect("session"))
    }

    #[test]
    fn chart_edits_reach_both_replicas_and_an_undo_is_one_step() {
        let mut hub = chart_hub();
        let welcome_a = join(&mut hub, "alice", Role::Editor);
        let welcome_b = join(&mut hub, "bob", Role::Editor);
        let mut sa = chart_session();
        let mut sb = chart_session();
        let mut a = CollabDoc::new("d");
        let mut b = CollabDoc::new("d");
        let _ = a.connect(&sa, "ja", ClientInfo { name: "a".into(), ver: "1".into() });
        let _ = b.connect(&sb, "jb", ClientInfo { name: "b".into(), ver: "1".into() });
        a.on_msg(&mut sa, ServerMsg::Welcome(welcome_a));
        b.on_msg(&mut sb, ServerMsg::Welcome(welcome_b));
        let chart = sa.doc().expect("doc").doc.slides[0].shapes[0].id.0;
        let before = sa.doc().expect("doc").doc.slides[0].shapes[0].clone();

        // One command becomes one whole chart value; both replicas end with the same chart.
        a.submit(&mut sa, "chart.type", &json!({"id": chart, "type": "pie"}), "a-1").expect("submit the type");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        a.submit(&mut sa, "chart.data", &json!({"id": chart, "categories": ["A", "B"], "series": [{"name": "S", "values": [1, 2]}]}), "a-2")
            .expect("submit the data");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (2, 2));
        let deckcraft_model::ShapeKind::Chart(ca) = &sa.doc().expect("doc").doc.slides[0].shapes[0].kind else { panic!("not a chart") };
        assert_eq!(ca.kind, deckcraft_model::chart::ChartType::Pie);
        assert_eq!(ca.categories, ["A", "B"]);
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc, "both replicas hold the same chart");

        // Each edit is its own step; two undos put the whole chart back on both replicas.
        for version in [3, 4] {
            a.undo().expect("request an undo");
            send_all(&mut hub, &mut a, "alice");
            deliver(&mut hub, &mut a, &mut sa, "alice");
            deliver(&mut hub, &mut b, &mut sb, "bob");
            assert_eq!((a.version(), b.version()), (version, version));
        }
        assert_eq!(sa.doc().expect("doc").doc.slides[0].shapes[0], before, "the whole chart came back");
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);
    }

    #[test]
    fn a_smartart_insertion_reaches_both_replicas_and_an_undo_restores_the_whole_shape() {
        let mut hub = hub();
        let welcome_a = join(&mut hub, "alice", Role::Editor);
        let welcome_b = join(&mut hub, "bob", Role::Editor);
        let mut sa = deck_session();
        let mut sb = deck_session();
        let mut a = CollabDoc::new("d");
        let mut b = CollabDoc::new("d");
        let _ = a.connect(&sa, "ja", ClientInfo { name: "a".into(), ver: "1".into() });
        let _ = b.connect(&sb, "jb", ClientInfo { name: "b".into(), ver: "1".into() });
        a.on_msg(&mut sa, ServerMsg::Welcome(welcome_a));
        b.on_msg(&mut sb, ServerMsg::Welcome(welcome_b));
        let count = sa.doc().expect("doc").doc.slides[0].shapes.len();

        // One command becomes one whole diagram value; both replicas create the same graphic.
        a.submit(&mut sa, "insert.smartArt", &json!({"kind": "hierarchy", "items": ["CEO", "\tCTO", "\t\tDev"], "rect": [40, 50, 400, 300]}), "a-1")
            .expect("submit the insertion");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (1, 1));
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc, "both replicas hold the same drawing");
        let inserted = sa.doc().expect("doc").doc.slides[0].shapes[count].clone();
        let d = inserted.diagram.as_ref().expect("a graphic");
        assert_eq!(d.kind, "hierarchy");
        let texts: Vec<&str> = d.nodes.iter().map(|n| n.text.as_str()).collect();
        assert_eq!(texts, ["CEO", "CTO", "Dev"]);

        // The undo removes the whole graphic as one step; the redo puts the whole shape back,
        // generated children and their ids included.
        a.undo().expect("request an undo");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (2, 2));
        assert_eq!(sa.doc().expect("doc").doc.slides[0].shapes.len(), count, "the graphic is gone");
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);
        a.redo().expect("request a redo");
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        deliver(&mut hub, &mut b, &mut sb, "bob");
        assert_eq!((a.version(), b.version()), (3, 3));
        assert_eq!(sa.doc().expect("doc").doc.slides[0].shapes[count], inserted, "the whole shape came back");
        assert_eq!(*sa.doc().expect("doc").doc, *sb.doc().expect("doc").doc);
    }

    #[test]
    fn a_viewer_cannot_insert_a_smartart_and_the_document_does_not_move() {
        let mut hub = hub();
        let _ = join(&mut hub, "alice", Role::Editor);
        let welcome_v = join(&mut hub, "victor", Role::Viewer);
        let mut sv = deck_session();
        let mut v = CollabDoc::new("d");
        let _ = v.connect(&sv, "jv", ClientInfo { name: "v".into(), ver: "1".into() });
        v.on_msg(&mut sv, ServerMsg::Welcome(welcome_v));
        assert_eq!(v.submit(&mut sv, "insert.smartArt", &json!({"kind": "list", "items": ["A"]}), "v-1"), Err(crate::LocalError::ReadOnly));

        // Even a hand-built operation never reaches the engine, and the version does not move.
        let (slide, _shape) = keys_of(&hub);
        let before = hub.session().document().expect("doc").clone();
        let step = OpEnvelope {
            op_id: "v-1".into(),
            doc: "d".into(),
            base_version: 0,
            kind: OpKind::Structural,
            target: Target { slide: Some(slide), ..Target::default() },
            payload: Payload::SmartArtInsert { diagram: json!({"kind": "list", "nodes": [{"text": "A", "level": 0}]}), rect: None },
            history: None,
            preview: None,
        };
        hub.receive("victor", ClientMsg::Op(step)).expect("receive");
        match hub.poll("victor").expect("answer") {
            ServerMsg::Rejected(r) => assert_eq!(r.code, RejectCode::Forbidden),
            other => panic!("{other:?}"),
        }
        assert_eq!(hub.version(), 0);
        assert_eq!(hub.session().document().expect("doc"), &before, "the document did not move");
    }

    #[test]
    fn the_authoritative_server_answers_the_smartart_nodes_query() {
        let mut hub = smartart_hub();
        let welcome_a = join(&mut hub, "alice", Role::Editor);
        let _ = join(&mut hub, "victor", Role::Viewer);
        let (slide, group) = keys_of(&hub);
        let target = Target { slide: Some(slide), shape: Some(group), ..Target::default() };
        let version = hub.version();
        let before = hub.session().document().expect("doc").clone();

        // The answer is the engine's own value at the authoritative head, and it goes to the
        // asker alone: no version moves and nobody else receives it.
        hub.receive("alice", ClientMsg::Query { id: "q-1".into(), cmd: "smartArt.nodes".into(), target: target.clone(), params: json!({}) })
            .expect("query");
        match hub.poll("alice").expect("answer") {
            ServerMsg::QueryResult { id, version: seen, result: Some(result), error: None } => {
                assert_eq!((id.as_str(), seen), ("q-1", version));
                assert_eq!(result["kind"], "blockList");
                let nodes: Vec<&str> = result["nodes"].as_array().expect("nodes").iter().map(|n| n["text"].as_str().unwrap()).collect();
                assert_eq!(nodes, ["Plan", "Scope", "Build"]);
            }
            other => panic!("{other:?}"),
        }
        assert!(hub.poll("victor").is_none(), "the answer is not broadcast");

        // A viewer may ask the same question; the document and its version stay put.
        hub.receive("victor", ClientMsg::Query { id: "q-2".into(), cmd: "smartArt.nodes".into(), target: target.clone(), params: json!({}) })
            .expect("query");
        assert!(matches!(hub.poll("victor").expect("answer"), ServerMsg::QueryResult { result: Some(_), .. }));
        assert_eq!(hub.version(), version);
        assert_eq!(hub.session().document().expect("doc"), &before);

        // A mutating command is refused as a query, and so are an engine query outside the wire
        // allowlist and an unknown command; all are an error in the answer, with no version move
        // and no rejected operation.
        for cmd in ["smartArt.layout", "file.recovery.list", "nope.unknown"] {
            hub.receive(
                "alice",
                ClientMsg::Query { id: format!("q-{cmd}"), cmd: cmd.into(), target: target.clone(), params: json!({"kind": "hierarchy"}) },
            )
            .expect("receive");
            match hub.poll("alice").expect("answer") {
                ServerMsg::QueryResult { result: None, error: Some(error), version: seen, .. } => {
                    assert_eq!(seen, version);
                    assert!(!error.retryable);
                }
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(hub.version(), version);
        assert_eq!(hub.session().document().expect("doc"), &before, "a query changes nothing");

        // The client path carries the same question and collects the answer.
        let mut sa = smartart_session();
        let mut a = CollabDoc::new("d");
        let _ = a.connect(&sa, "ja", ClientInfo { name: "a".into(), ver: "1".into() });
        a.on_msg(&mut sa, ServerMsg::Welcome(welcome_a));
        a.query("q-3", "smartArt.nodes", target, json!({}));
        send_all(&mut hub, &mut a, "alice");
        deliver(&mut hub, &mut a, &mut sa, "alice");
        let answer = a.take_query_answer().expect("the client collected the answer");
        assert_eq!(answer.id, "q-3");
        assert_eq!(answer.version, version);
        assert!(answer.error.is_none());
        assert_eq!(answer.result.expect("result")["kind"], "blockList");
    }
}
