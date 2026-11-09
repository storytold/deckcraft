//! The client's side of one collaboration session, without a transport.
//!
//! The application drives this state machine and moves the returned messages over whatever link
//! it has (a WebSocket on the desktop, the browser's WebSocket in wasm). Everything that can be
//! wrong about ordering, retries and reconnect is decided here and tested headless (ADR 0001,
//! 0003, 0008).
//!
//! Rules it enforces:
//!
//! - **At most `max_in_flight` operations** are unacknowledged at once; the rest wait in order.
//! - **A retryable refusal** (`stale`, `lease`) puts the operation back at the front of the queue
//!   and asks for a resync: the client never patches its state by guessing.
//! - **A version gap** is a resync, not a silent skip.
//! - **A viewer cannot submit.** The server checks too; this only keeps the UI honest.

use std::collections::VecDeque;

use deckcraft_collab_proto::{ClientInfo, ClientMsg, Hello, Lease, OpEnvelope, PROTO_VERSION, RejectCode, Role, ServerMsg, SnapshotMode};

/// Why a submitted operation was not accepted by the client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubmitError {
    /// A viewer may not change the document (ADR 0008).
    ReadOnly,
    /// Too many operations are waiting; the user is typing faster than the link moves.
    Busy,
}

/// What the application should do after [`ClientSession::on_msg`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Nothing to do beyond the state change.
    None,
    /// The local preview for this operation is now authoritative; drop it from the UI.
    Settled,
    /// The local preview must be rolled back and rebuilt from the server's state.
    Rollback,
}

/// Connection state of one session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    Down,
    Connecting,
    Live,
}

/// Client state for one document session.
#[derive(Debug)]
pub struct ClientSession {
    file_id: String,
    actor_id: Option<String>,
    role: Option<Role>,
    lease: Option<Lease>,
    state: Link,
    version: u64,
    in_flight: VecDeque<OpEnvelope>,
    queued: VecDeque<OpEnvelope>,
    max_in_flight: usize,
    max_queued: usize,
    resync_wanted: bool,
    last_error: Option<String>,
}

impl ClientSession {
    pub fn new(file_id: impl Into<String>) -> Self {
        ClientSession {
            file_id: file_id.into(),
            actor_id: None,
            role: None,
            lease: None,
            state: Link::Down,
            version: 0,
            in_flight: VecDeque::new(),
            queued: VecDeque::new(),
            max_in_flight: 8,
            max_queued: 256,
            resync_wanted: false,
            last_error: None,
        }
    }

    pub fn file_id(&self) -> &str {
        &self.file_id
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn role(&self) -> Option<Role> {
        self.role
    }

    pub fn actor_id(&self) -> Option<&str> {
        self.actor_id.as_deref()
    }

    pub fn lease(&self) -> Option<&Lease> {
        self.lease.as_ref()
    }

    pub fn link(&self) -> Link {
        self.state
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight.len()
    }

    pub fn queued(&self) -> usize {
        self.queued.len()
    }

    /// Begin a connection: the next outgoing message is the handshake (ADR 0008: the token
    /// travels there and nowhere else).
    pub fn connect(&mut self, jwt: impl Into<String>, client: ClientInfo) -> ClientMsg {
        self.state = Link::Connecting;
        self.last_error = None;
        ClientMsg::Hello(Hello {
            v: PROTO_VERSION,
            app: "deckcraft".into(),
            doc: self.file_id.clone(),
            jwt: jwt.into(),
            client,
            last_version: Some(self.version),
        })
    }

    /// Submit a local operation. A viewer is refused here and on the server.
    pub fn submit(&mut self, op: OpEnvelope) -> Result<(), SubmitError> {
        if self.role.is_some_and(|role| !role.may_write()) {
            return Err(SubmitError::ReadOnly);
        }
        if self.queued.len() >= self.max_queued {
            return Err(SubmitError::Busy);
        }
        self.queued.push_back(op);
        Ok(())
    }

    /// The next message the application should send, if any.
    pub fn next_outgoing(&mut self) -> Option<ClientMsg> {
        if self.state != Link::Live {
            return None;
        }
        if self.resync_wanted {
            self.resync_wanted = false;
            return Some(ClientMsg::Resync { last_version: self.version });
        }
        if self.in_flight.len() >= self.max_in_flight {
            return None;
        }
        let mut op = self.queued.pop_front()?;
        // The head this client knows is the base the server orders the operation against; a
        // structural operation is only accepted at the head (ADR 0004).
        op.base_version = self.version;
        self.in_flight.push_back(op.clone());
        Some(ClientMsg::Op(op))
    }

    /// Handle one message from the server.
    pub fn on_msg(&mut self, msg: ServerMsg) -> Effect {
        match msg {
            ServerMsg::Welcome(welcome) => {
                self.state = Link::Live;
                self.actor_id = Some(welcome.actor_id);
                self.role = Some(welcome.role);
                self.lease = welcome.lease;
                // A gap between our version and the server's means a resync, not a guess. The
                // local document is still at `self.version`, so that is the version the resync
                // must ask the tail from: adopting the server head here would ask for a tail
                // after the head and leave the replica behind while claiming to be current.
                if welcome.version > self.version {
                    self.resync_wanted = true;
                } else {
                    self.version = self.version.max(welcome.version);
                }
                Effect::Rollback
            }
            ServerMsg::Applied(applied) => {
                let settled = self.in_flight.iter().position(|op| op.op_id == applied.op_id);
                if let Some(index) = settled {
                    self.in_flight.remove(index);
                }
                if applied.version > self.version + 1 {
                    // Something between was missed: ask for the tail instead of skipping.
                    self.resync_wanted = true;
                    return Effect::Rollback;
                }
                self.version = self.version.max(applied.version);
                Effect::Settled
            }
            ServerMsg::Rejected(rejected) => {
                let position = self.in_flight.iter().position(|op| op.op_id == rejected.op_id);
                let op = position.and_then(|i| self.in_flight.remove(i));
                self.last_error = Some(rejected.message.clone());
                if rejected.retryable {
                    // Put the operation back where it was and let the resync rebuild the state.
                    if let Some(op) = op {
                        self.queued.push_front(op);
                    }
                    self.resync_wanted = true;
                    Effect::Rollback
                } else {
                    if rejected.code == RejectCode::Auth || rejected.code == RejectCode::Lease {
                        self.state = Link::Down;
                    }
                    Effect::Rollback
                }
            }
            ServerMsg::Snapshot(snapshot) => {
                self.version = self.version.max(snapshot.version);
                // Operations the base or the tail already includes need no answer. A base at a
                // later version replaces the replica; the tail above it is replayed by the host.
                let floor = snapshot.base.as_ref().map(|base| base.version).unwrap_or(self.version);
                let tail: Vec<String> = snapshot.tail.iter().map(|applied| applied.op_id.clone()).collect();
                let covered: Vec<usize> =
                    self.in_flight.iter().enumerate().filter(|(_, op)| op.base_version < floor || tail.contains(&op.op_id)).map(|(i, _)| i).collect();
                for index in covered.into_iter().rev() {
                    self.in_flight.remove(index);
                }
                if snapshot.snapshot.mode == SnapshotMode::Base {
                    // The replica is replaced from the server's base; edits that never reached the
                    // server are not rebased onto it, so they are dropped rather than applied at a
                    // offset the base does not have.
                    self.queued.clear();
                }
                Effect::Rollback
            }
            ServerMsg::Presence(_) => Effect::None,
            ServerMsg::Pong => Effect::None,
            ServerMsg::CommitResult(_) => Effect::None,
            ServerMsg::Error(error) => {
                self.last_error = Some(error.message);
                self.state = Link::Down;
                Effect::Rollback
            }
        }
    }

    /// Drop everything for a reconnect: unacknowledged operations stay queued with their ids, so
    /// the server dedups them if they did arrive (ADR 0003). They go back to the **front** in
    /// their original order: after the handshake the client must resend them before newer work.
    pub fn disconnected(&mut self) {
        self.state = Link::Down;
        self.lease = None;
        self.role = None;
        while let Some(op) = self.in_flight.pop_back() {
            self.queued.push_front(op);
        }
        self.resync_wanted = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_collab_proto::{Applied, Capabilities, Delta, Hint, Limits, OpKind, Payload, Rejected, SnapshotMsg, SnapshotRef, Target, Welcome};

    fn op(id: &str, base: u64) -> OpEnvelope {
        OpEnvelope {
            op_id: id.into(),
            doc: "d".into(),
            base_version: base,
            kind: OpKind::Command,
            target: Target::default(),
            payload: Payload::ShapeMove { x: 0.0, y: 0.0 },
            history: None,
        }
    }

    fn welcome(version: u64, role: Role) -> ServerMsg {
        ServerMsg::Welcome(Welcome {
            v: PROTO_VERSION,
            session_id: "s".into(),
            actor_id: "a".into(),
            role,
            lease: None,
            version,
            snapshot: SnapshotRef { mode: SnapshotMode::BaseAndTail, cursor: "tail@0".into() },
            capabilities: Capabilities { kinds: vec![OpKind::Command], limits: Limits::default(), commands: vec![] },
        })
    }

    fn applied(id: &str, version: u64) -> ServerMsg {
        ServerMsg::Applied(Applied { op_id: id.into(), version, actor_id: "a".into(), command: None, delta: Delta::default() })
    }

    #[test]
    fn a_viewer_cannot_submit_and_the_server_is_not_consulted() {
        let mut c = ClientSession::new("d");
        c.on_msg(welcome(0, Role::Viewer));
        assert_eq!(c.submit(op("o1", 0)), Err(SubmitError::ReadOnly));
        assert_eq!(c.queued(), 0);
    }

    #[test]
    fn operations_wait_until_the_link_is_live() {
        let mut c = ClientSession::new("d");
        c.submit(op("o1", 0)).expect("queue");
        assert!(c.next_outgoing().is_none(), "nothing goes out before welcome");
        c.on_msg(welcome(0, Role::Editor));
        assert!(matches!(c.next_outgoing(), Some(ClientMsg::Op(op)) if op.op_id == "o1"));
        assert_eq!(c.in_flight(), 1);
    }

    #[test]
    fn applied_settles_the_operation_and_advances_the_version() {
        let mut c = ClientSession::new("d");
        c.on_msg(welcome(0, Role::Editor));
        c.submit(op("o1", 0)).expect("queue");
        c.next_outgoing().expect("sent");
        assert_eq!(c.on_msg(applied("o1", 1)), Effect::Settled);
        assert_eq!(c.version(), 1);
        assert_eq!(c.in_flight(), 0);
    }

    #[test]
    fn a_retryable_refusal_requeues_the_operation_and_asks_for_a_resync() {
        let mut c = ClientSession::new("d");
        c.on_msg(welcome(0, Role::Editor));
        c.submit(op("o1", 0)).expect("queue");
        c.next_outgoing().expect("sent");
        let rejected = ServerMsg::Rejected(Rejected {
            op_id: "o1".into(),
            code: RejectCode::Stale,
            message: "moved".into(),
            retryable: true,
            hint: Some(Hint { version: 3 }),
        });
        assert_eq!(c.on_msg(rejected), Effect::Rollback);
        assert_eq!(c.queued(), 1, "the operation is not lost");
        assert_eq!(c.version(), 0, "the client does not guess the new version");
        assert!(matches!(c.next_outgoing(), Some(ClientMsg::Resync { last_version: 0 })));
    }

    #[test]
    fn a_forbidden_operation_is_dropped_not_retried() {
        let mut c = ClientSession::new("d");
        c.on_msg(welcome(0, Role::Editor));
        c.submit(op("o1", 0)).expect("queue");
        c.next_outgoing().expect("sent");
        let rejected =
            ServerMsg::Rejected(Rejected { op_id: "o1".into(), code: RejectCode::Forbidden, message: "locked".into(), retryable: false, hint: None });
        assert_eq!(c.on_msg(rejected), Effect::Rollback);
        assert_eq!(c.queued(), 0);
        assert_eq!(c.in_flight(), 0);
        assert_eq!(c.last_error(), Some("locked"));
    }

    #[test]
    fn a_version_gap_asks_for_the_tail_instead_of_skipping() {
        let mut c = ClientSession::new("d");
        c.on_msg(welcome(1, Role::Editor));
        assert_eq!(c.version(), 0, "the welcome head is not adopted before the tail arrives");
        assert_eq!(c.on_msg(applied("o1", 4)), Effect::Rollback);
        assert!(matches!(c.next_outgoing(), Some(ClientMsg::Resync { last_version: 0 })));
        assert_eq!(c.version(), 0, "the guessed version is not adopted");
    }

    #[test]
    fn a_reconnect_asks_for_the_tail_from_the_version_the_replica_has() {
        let mut c = ClientSession::new("d");
        c.on_msg(welcome(0, Role::Editor));
        c.on_msg(applied("o1", 1));
        c.on_msg(applied("o2", 2));
        assert_eq!(c.version(), 2);
        c.disconnected();
        let _ = c.connect("jwt", ClientInfo { name: "t".into(), ver: "1".into() });
        c.on_msg(welcome(5, Role::Editor));
        assert_eq!(c.version(), 2, "the server head is not adopted before the tail arrives");
        assert!(
            matches!(c.next_outgoing(), Some(ClientMsg::Resync { last_version: 2 })),
            "the resync names the version the local document actually has"
        );
    }

    #[test]
    fn a_queued_operation_is_sent_against_the_head_the_client_knows() {
        let mut c = ClientSession::new("d");
        c.on_msg(welcome(0, Role::Editor));
        c.submit(op("o1", 9)).expect("queue");
        let sent = c.next_outgoing().expect("sent");
        assert!(matches!(sent, ClientMsg::Op(ref op) if op.base_version == 0), "the stale base is replaced at send");

        let snapshot = ServerMsg::Snapshot(SnapshotMsg {
            version: 5,
            snapshot: SnapshotRef { mode: SnapshotMode::BaseAndTail, cursor: "tail@5".into() },
            tail: vec![],
            base: None,
        });
        c.on_msg(snapshot);
        c.submit(op("o2", 0)).expect("queue");
        let sent = c.next_outgoing().expect("sent");
        assert!(matches!(sent, ClientMsg::Op(ref op) if op.op_id == "o2" && op.base_version == 5));
    }

    #[test]
    fn reconnect_keeps_unanswered_operations_for_dedup() {
        let mut c = ClientSession::new("d");
        c.on_msg(welcome(0, Role::Editor));
        c.submit(op("o1", 0)).expect("queue");
        c.next_outgoing().expect("sent");
        c.disconnected();
        assert_eq!(c.link(), Link::Down);
        assert_eq!(c.in_flight(), 0);
        assert_eq!(c.queued(), 1, "an unanswered operation is kept for the resend");
        let _ = c.connect("jwt", ClientInfo { name: "t".into(), ver: "1".into() });
        c.on_msg(welcome(0, Role::Editor));
        let sent = c.next_outgoing().expect("resent");
        assert!(matches!(sent, ClientMsg::Op(ref op) if op.op_id == "o1"), "same opId, so the server dedups it");
    }

    #[test]
    fn a_snapshot_covers_operations_it_already_includes() {
        let mut c = ClientSession::new("d");
        c.on_msg(welcome(0, Role::Editor));
        for id in ["o1", "o2"] {
            c.submit(op(id, 0)).expect("queue");
            c.next_outgoing().expect("sent");
        }
        let snapshot = ServerMsg::Snapshot(SnapshotMsg {
            version: 2,
            snapshot: SnapshotRef { mode: SnapshotMode::BaseAndTail, cursor: "base@2".into() },
            tail: vec![],
            base: None,
        });
        assert_eq!(c.on_msg(snapshot), Effect::Rollback);
        assert_eq!(c.in_flight(), 0, "both operations are in the base");
        assert_eq!(c.version(), 2);
    }

    #[test]
    fn a_base_snapshot_replaces_the_replica_at_its_version() {
        use deckcraft_collab_proto::{BaseSnapshot, SnapshotKeys};
        let mut c = ClientSession::new("d");
        c.on_msg(welcome(9, Role::Editor));
        assert_eq!(c.version(), 0, "the head is not adopted before the base arrives");
        c.submit(op("q1", 0)).expect("queue");
        assert_eq!(c.queued(), 1);
        let snapshot = ServerMsg::Snapshot(SnapshotMsg {
            version: 9,
            snapshot: SnapshotRef { mode: SnapshotMode::Base, cursor: "base@6".into() },
            tail: vec![
                Applied { op_id: "o7".into(), version: 7, actor_id: "a".into(), command: None, delta: Delta::default() },
                Applied { op_id: "o8".into(), version: 8, actor_id: "a".into(), command: None, delta: Delta::default() },
                Applied { op_id: "o9".into(), version: 9, actor_id: "a".into(), command: None, delta: Delta::default() },
            ],
            base: Some(BaseSnapshot { version: 6, bytes_base64: Some("UEsD".into()), keys: SnapshotKeys::default() }),
        });
        assert_eq!(c.on_msg(snapshot), Effect::Rollback);
        assert_eq!(c.version(), 9, "after the base and its tail the client is at the head");
        assert_eq!(c.queued(), 0, "an edit from the old replica is not applied onto the base");
    }

    #[test]
    fn the_handshake_carries_the_last_seen_version() {
        let mut c = ClientSession::new("d");
        c.on_msg(welcome(0, Role::Editor));
        for version in 1..=7 {
            c.on_msg(applied(&format!("o{version}"), version));
        }
        assert_eq!(c.version(), 7);
        let msg = c.connect("jwt", ClientInfo { name: "deckcraft-web".into(), ver: "0.1".into() });
        assert!(matches!(msg, ClientMsg::Hello(_)), "the first message is the handshake");
        if let ClientMsg::Hello(hello) = msg {
            assert_eq!(hello.last_version, Some(7));
            assert_eq!(hello.app, "deckcraft");
            assert_eq!(hello.doc, "d");
        }
    }
}
