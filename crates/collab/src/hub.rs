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
}

impl Hub {
    pub fn new(session: DocSession) -> Self {
        Hub { session, peers: BTreeMap::new(), out: HashMap::new(), last_applied: None }
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
                    "text.set".into(),
                    "text.insert".into(),
                    "text.delete".into(),
                    "shape.insert".into(),
                    "insert.picture".into(),
                    "edit.delete".into(),
                    "slide.new".into(),
                    "slide.delete".into(),
                    "slide.move".into(),
                    "arrange.group".into(),
                    "arrange.ungroup".into(),
                    "arrange.reorder".into(),
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
                self.send(actor, ServerMsg::Snapshot(snapshot));
                Ok(())
            }
            ClientMsg::Undo => self.history(actor, true),
            ClientMsg::Redo => self.history(actor, false),
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
                payload: Payload::TextInsert { at: 1, text: ch.into() },
                history: None,
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
        a.submit(&sa, "shape.insert", &json!({"preset": "rect", "rect": [50, 50, 40, 30], "text": "hi"}), "ins-1").expect("submit");
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
        a.submit(&sa, "shape.move", &json!({"ids": [shape], "x": 250.0, "y": 100.0}), "a-1").expect("submit");
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
        a.submit(&sa, "slide.new", &json!({"layout": "blank", "at": 1}), "a-2").expect("submit slide");
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
}
