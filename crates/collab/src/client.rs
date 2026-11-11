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

use base64::Engine as _;
use deckcraft_collab_proto::{
    BaseChunks, ClientInfo, ClientMsg, Hello, Lease, OpEnvelope, PROTO_VERSION, RejectCode, Role, ServerMsg, SnapshotMode, SnapshotMsg,
};

/// Why a submitted operation was not accepted by the client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubmitError {
    /// A viewer may not change the document (ADR 0008).
    ReadOnly,
    /// Too many operations are waiting; the user is typing faster than the link moves.
    Busy,
    /// The replica is being rebuilt from a base the server is sending in chunks; an edit made now
    /// would be normalised against a document the room no longer has.
    Rebuilding,
}

/// Why a commit was not asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitError {
    /// A viewer may not commit the file (ADR 0008, 0009).
    ReadOnly,
    /// The link is not live: the request has nowhere to go.
    Offline,
}

impl std::fmt::Display for CommitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommitError::ReadOnly => f.write_str("view only: you cannot commit the presentation"),
            CommitError::Offline => f.write_str("the session is offline: Save a copy keeps your work"),
        }
    }
}

/// A host's Save: the cabinet version the file was opened from, or the one the last commit was
/// accepted with. `None` leaves the check to the server, which compares against the version it
/// restored from `meta.json` — the version this room was opened from.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CommitRequest {
    expected: Option<String>,
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
    /// The version the host's document actually holds. It is below [`Self::version`] only while a
    /// chained base is being assembled; a reconnect asks for the catch-up from this version, so
    /// the server answers with the base the chain belongs to.
    replica_version: u64,
    in_flight: VecDeque<OpEnvelope>,
    queued: VecDeque<OpEnvelope>,
    max_in_flight: usize,
    max_queued: usize,
    resync_wanted: bool,
    /// A `Resync` is on the wire and its snapshot has not arrived. Nothing else may be sent while
    /// it is outstanding: an operation sent after it carries the old `base_version`, and the
    /// snapshot that answers the resync would cover it as "already included" although the server
    /// never applied it.
    resync_in_flight: bool,
    /// The version the host rewound its replica to while a `Resync` was on the wire. That request
    /// was asked from the version the replica held before the rewind, so the snapshot that answers
    /// it starts above the rewound version and does not carry the steps the replica now misses.
    /// It is kept until a snapshot actually covers the rewound version.
    rewind_pending: Option<u64>,
    last_error: Option<String>,
    /// A base the server is sending as a chain of chunks (RFC §4.5.1): the prefix assembled so
    /// far and the index the client is missing.
    base_download: Option<BaseDownload>,
    /// The chunk the client has asked for and not yet been answered. Cleared when the socket
    /// drops, so the same index is asked again and the chain resumes instead of restarting.
    base_request: Option<u32>,
    /// The snapshot whose base is being assembled: its key map, tail and head reach the host only
    /// when the base is whole (`take_ready_base`).
    pending_snapshot: Option<SnapshotMsg>,
    /// The last snapshot was refused because it did not cover a rewind. Kept for the host, which
    /// must skip it as well: installing its base or tail would put the replica past the steps the
    /// rewind left missing even though this state machine did not move (see
    /// [`Self::take_refused_snapshot`]).
    snapshot_refused: bool,
    /// A base that finished assembling, for the host to install.
    ready_base: Option<ReadyBase>,
    /// Journal steps that arrived while the base was assembling. They are not applied to the
    /// stale replica; the catch-up tail asked for after the base is installed carries them.
    missed_while_rebuilding: bool,
    /// A commit the host asked for and this client has not sent yet (ADR 0009).
    commit_pending: Option<CommitRequest>,
    /// Whether the commit is on the wire: a Save while it is there is the same request, not a
    /// second file version.
    commit_in_flight: bool,
    /// The cabinet's file version the last commit was accepted with; the next commit expects it.
    file_version: Option<String>,
}

/// A base assembled from a chain of chunks, ready for the host to install in place of its replica
/// (RFC §4.5.1). Its snapshot carries the key map, the tail above the base and the head; the
/// bytes are the native `.deckcraft` file, already checked against the digest the server named.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadyBase {
    pub snapshot: SnapshotMsg,
    pub bytes: Vec<u8>,
}

/// The prefix of a chained base, and where the client is in the chain.
#[derive(Clone, Debug, PartialEq)]
struct BaseDownload {
    version: u64,
    sha256: String,
    len: usize,
    chunk_size: usize,
    count: usize,
    next: u32,
    bytes: Vec<u8>,
}

impl BaseDownload {
    /// Start assembling a chain the snapshot announced; a chain outside the protocol's ceilings is
    /// refused before the replica allocates anything for it.
    fn start(version: u64, chain: &BaseChunks) -> Result<Self, String> {
        if !chain.is_well_formed() {
            return Err("the session base is announced as a chain this client cannot accept".into());
        }
        Ok(BaseDownload {
            version,
            sha256: chain.sha256.to_ascii_lowercase(),
            len: chain.len as usize,
            chunk_size: chain.chunk_size as usize,
            count: chain.count as usize,
            next: 0,
            bytes: Vec::with_capacity((chain.len as usize).min(4 << 20)),
        })
    }

    /// Whether `chain` describes the base this download is already assembling, so a second
    /// snapshot for it continues at the index the client is missing instead of starting over.
    fn continues(&self, version: u64, chain: &BaseChunks) -> bool {
        self.version == version
            && self.sha256 == chain.sha256.to_ascii_lowercase()
            && self.len == chain.len as usize
            && self.chunk_size == chain.chunk_size as usize
            && self.count == chain.count as usize
    }

    /// The decoded size the chunk at `index` must have.
    fn chunk_len(&self, index: u32) -> usize {
        let start = index as usize * self.chunk_size;
        self.len.saturating_sub(start).min(self.chunk_size)
    }

    fn finished(&self) -> bool {
        self.next as usize >= self.count
    }
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
            replica_version: 0,
            in_flight: VecDeque::new(),
            queued: VecDeque::new(),
            max_in_flight: 8,
            max_queued: 256,
            resync_wanted: false,
            resync_in_flight: false,
            rewind_pending: None,
            last_error: None,
            base_download: None,
            base_request: None,
            pending_snapshot: None,
            snapshot_refused: false,
            ready_base: None,
            missed_while_rebuilding: false,
            commit_pending: None,
            commit_in_flight: false,
            file_version: None,
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
            // The version the document actually holds, not the head of a base still being
            // assembled: the catch-up must start where the replica is.
            last_version: Some(self.replica_version),
        })
    }

    /// Submit a local operation. A viewer is refused here and on the server.
    pub fn submit(&mut self, op: OpEnvelope) -> Result<(), SubmitError> {
        if self.role.is_some_and(|role| !role.may_write()) {
            return Err(SubmitError::ReadOnly);
        }
        if self.pending_snapshot.is_some() {
            return Err(SubmitError::Rebuilding);
        }
        if self.queued.len() >= self.max_queued {
            return Err(SubmitError::Busy);
        }
        self.queued.push_back(op);
        Ok(())
    }

    /// Ask the server to commit the room as a new cabinet file version (ADR 0009). Only a writer
    /// may ask, and a Save while a commit is already on its way is the same request: one file
    /// version, not two. `expected` is the cabinet version the file was opened from, or the one
    /// the last commit was accepted with; `None` leaves the check to the server, which compares
    /// against the version it restored from `meta.json`.
    pub fn commit(&mut self, expected: Option<String>) -> Result<(), CommitError> {
        if self.role.is_some_and(|role| !role.may_write()) {
            return Err(CommitError::ReadOnly);
        }
        if self.state != Link::Live {
            return Err(CommitError::Offline);
        }
        if self.commit_pending.is_none() && !self.commit_in_flight {
            self.commit_pending = Some(CommitRequest { expected });
        }
        Ok(())
    }

    /// Whether a commit is queued or on the wire (a Save now would be the same request).
    pub fn commit_in_flight(&self) -> bool {
        self.commit_pending.is_some() || self.commit_in_flight
    }

    /// The cabinet file version the last commit was accepted with. The next commit expects it,
    /// so a file changed outside the session after that commit is a `409` (ADR 0009 §4).
    pub fn file_version(&self) -> Option<&str> {
        self.file_version.as_deref()
    }

    /// The next message the application should send, if any.
    pub fn next_outgoing(&mut self) -> Option<ClientMsg> {
        if self.state != Link::Live {
            return None;
        }
        if self.resync_wanted && !self.resync_in_flight {
            // One resync on the wire at a time. A gap reported while an answer is in flight stays
            // `resync_wanted` and is asked for after this snapshot: a second request would put
            // two snapshots in flight, and the later one could drop an operation the first one
            // released as "covered" although the server never applied it.
            self.resync_wanted = false;
            self.resync_in_flight = true;
            return Some(ClientMsg::Resync { last_version: self.replica_version });
        }
        // A chained base is fetched one chunk at a time, before any new work: the replica cannot
        // address the room until the base is installed. One request is outstanding at a time.
        if self.base_request.is_none()
            && let Some(download) = self.base_download.as_ref()
            && !download.finished()
        {
            self.base_request = Some(download.next);
            return Some(ClientMsg::BaseChunk { version: download.version, index: download.next });
        }
        if self.resync_in_flight {
            // Held until the snapshot arrives: see the field's comment.
            return None;
        }
        if self.pending_snapshot.is_some() {
            // Nothing may be submitted while the base is assembling, and a resend of an operation
            // from the old replica would be normalised against a document the room no longer has.
            return None;
        }
        // A commit waits until every edit is acknowledged: the file the server builds is the one
        // the user pressed Save on, not one missing the last keystroke.
        if self.queued.is_empty()
            && self.in_flight.is_empty()
            && let Some(request) = self.commit_pending.take()
        {
            self.commit_in_flight = true;
            return Some(ClientMsg::Commit { expected_file_version: request.expected });
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
                self.resync_in_flight = false;
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
                if self.pending_snapshot.is_some() {
                    // The replica is being rebuilt from a chained base. This step is either in the
                    // snapshot's tail or above it; either way it must not move a replica that is
                    // still the old document, so it is collected by the catch-up asked for when
                    // the base is installed.
                    self.missed_while_rebuilding = true;
                    return Effect::None;
                }
                if applied.version > self.version + 1 {
                    // Something between was missed: ask for the tail instead of skipping.
                    self.resync_wanted = true;
                    return Effect::Rollback;
                }
                self.version = self.version.max(applied.version);
                self.replica_version = self.replica_version.max(applied.version);
                Effect::Settled
            }
            ServerMsg::Rejected(rejected) => {
                if rejected.op_id == "commit" {
                    // The cabinet refused this commit (a `409` after an external change, a build
                    // failure): the answer is final for the request, so a later Save may ask
                    // again. The expected version is kept untouched — a retry against a file that
                    // moved on must be refused again, never overwrite it (ADR 0009 §4).
                    self.commit_in_flight = false;
                }
                if self.pending_snapshot.is_some() && rejected.op_id.starts_with("base@") {
                    // The server no longer serves the base this chain belongs to (compaction moved
                    // it). The partial prefix is useless, so the client drops it and asks for a new
                    // snapshot instead of retrying the same index forever.
                    self.last_error = Some(rejected.message.clone());
                    self.base_download = None;
                    self.base_request = None;
                    self.pending_snapshot = None;
                    self.resync_wanted = true;
                    return Effect::Rollback;
                }
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
                // The host asks this after every snapshot: a refusal is final for the answer and
                // must not be mistaken for the previous one.
                self.snapshot_refused = false;
                // The resync is answered: operations may be sent again, against the head the
                // snapshot installs (the queued ones take their `base_version` at send time).
                self.resync_in_flight = false;
                if let Some(rewound) = self.rewind_pending.take()
                    && !snapshot_covers(&snapshot, rewound)
                {
                    // The host put its replica back to `rewound` while this request was on the
                    // wire, so the answer was asked from a higher version: its base is older than
                    // the replica and its tail starts above it. Applying it would leave a hole
                    // between them and advance the replica past the steps it is missing, so the
                    // replica stays where it is and the tail is asked for again from there.
                    self.snapshot_refused = true;
                    self.resync_wanted = true;
                    return Effect::Rollback;
                }
                // A gap signalled while this answer was on its way is covered by it: the snapshot
                // and the tail above the replica are the room's state up to the same head, so no
                // second resync is owed for it.
                self.resync_wanted = false;
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
                // A base larger than the frame limit travels as a chain of chunks: the host gets
                // this snapshot only once the base is whole (`take_ready_base`), and until then
                // the replica is the old document.
                match snapshot.base.as_ref().and_then(|base| base.chunks.clone().map(|chain| (base.version, chain))) {
                    Some((version, chain)) => {
                        // A second snapshot for the base this client is already assembling (the
                        // catch-up after a reconnect) continues the chain at the missing index.
                        let keep = self.base_download.as_ref().is_some_and(|download| download.continues(version, &chain));
                        if !keep {
                            match BaseDownload::start(version, &chain) {
                                Ok(download) => {
                                    self.base_download = Some(download);
                                    self.base_request = None;
                                }
                                Err(message) => {
                                    self.base_download = None;
                                    self.base_request = None;
                                    self.pending_snapshot = None;
                                    self.missed_while_rebuilding = false;
                                    self.last_error = Some(message);
                                    return Effect::Rollback;
                                }
                            }
                        }
                        self.pending_snapshot = Some(snapshot);
                    }
                    None => {
                        self.base_download = None;
                        self.base_request = None;
                        self.pending_snapshot = None;
                        self.missed_while_rebuilding = false;
                        // The host applies this snapshot (base and tail) as it arrives.
                        self.replica_version = self.replica_version.max(snapshot.version);
                    }
                }
                Effect::Rollback
            }
            ServerMsg::BaseChunk { version, index, count, data } => self.on_base_chunk(version, index, count, &data),
            ServerMsg::Presence(_) => Effect::None,
            ServerMsg::QueryResult { .. } => Effect::None,
            ServerMsg::Pong => Effect::None,
            ServerMsg::CommitResult(result) => {
                self.commit_in_flight = false;
                if let Some(version) = &result.file_version {
                    self.file_version = Some(version.clone());
                }
                Effect::None
            }
            ServerMsg::Error(error) => {
                self.commit_in_flight = false;
                self.last_error = Some(error.message);
                self.state = Link::Down;
                Effect::Rollback
            }
        }
    }

    /// Drop everything for a reconnect: unacknowledged operations stay queued with their ids, so
    /// the server dedups them if they did arrive (ADR 0003). They go back to the **front** in
    /// their original order: after the handshake the client must resend them before newer work.
    ///
    /// A chained base keeps its assembled prefix: only the outstanding request is forgotten, so
    /// the next connection asks for the same chunk and the chain resumes instead of restarting.
    pub fn disconnected(&mut self) {
        self.state = Link::Down;
        self.lease = None;
        self.role = None;
        while let Some(op) = self.in_flight.pop_back() {
            self.queued.push_front(op);
        }
        self.resync_wanted = false;
        self.resync_in_flight = false;
        self.rewind_pending = None;
        self.snapshot_refused = false;
        self.base_request = None;
        // A commit that was sent has no answer to resend: the user is told it did not finish and
        // presses Save again. One that never left stays queued for the next connection.
        self.commit_in_flight = false;
    }

    /// One chunk of a chained base: append it, ask for the next one, and hand the base to the host
    /// when the chain is whole.
    fn on_base_chunk(&mut self, version: u64, index: u32, count: u32, data: &str) -> Effect {
        if self.base_download.as_ref().is_none_or(|download| download.version != version) {
            // An answer for a chain this client abandoned; the next chunk request is the truth.
            return Effect::None;
        }
        self.base_request = None;
        let mut failure = None;
        {
            let Some(download) = self.base_download.as_mut() else { return Effect::None };
            if download.count != count as usize {
                failure = Some(format!("the base at version {version} changed its chain while it was being sent"));
            } else if index == download.next {
                match base64::engine::general_purpose::STANDARD.decode(data) {
                    Ok(bytes) if bytes.len() == download.chunk_len(index) && download.bytes.len() + bytes.len() <= download.len => {
                        download.bytes.extend_from_slice(&bytes);
                        download.next += 1;
                    }
                    Ok(_) => failure = Some("a base chunk is not the size the chain announced".into()),
                    Err(_) => failure = Some("a base chunk is not base64".into()),
                }
            }
            // Any other index is a duplicate or a chunk from the future; the next request fixes it.
        }
        if let Some(message) = failure {
            return self.abandon_base(message);
        }
        if self.base_download.as_ref().is_some_and(BaseDownload::finished) { self.finish_base() } else { Effect::None }
    }

    /// The whole base arrived: check its size and digest, clear the chain metadata from the
    /// snapshot and hand it to the host through [`Self::take_ready_base`].
    fn finish_base(&mut self) -> Effect {
        let Some(download) = self.base_download.take() else { return Effect::None };
        let Some(mut snapshot) = self.pending_snapshot.take() else {
            self.missed_while_rebuilding = false;
            return self.abandon_base("the base finished without the snapshot it belongs to");
        };
        if download.bytes.len() != download.len {
            self.missed_while_rebuilding = false;
            return self.abandon_base(format!("the base is {} bytes, not {}", download.bytes.len(), download.len));
        }
        if crate::media::sha256_hex(&download.bytes) != download.sha256 {
            self.missed_while_rebuilding = false;
            return self.abandon_base("the assembled base does not match the digest the server announced");
        }
        if let Some(base) = snapshot.base.as_mut() {
            base.chunks = None;
            base.bytes_base64 = None;
        }
        self.replica_version = snapshot.version;
        self.ready_base = Some(ReadyBase { snapshot, bytes: download.bytes });
        if self.missed_while_rebuilding {
            // Steps arrived while the base was assembling and were not applied to the stale
            // replica; ask for the tail above the base now that it is installed.
            self.missed_while_rebuilding = false;
            self.resync_wanted = true;
        }
        Effect::Rollback
    }

    /// Give up on a chain: the replica stays at the old document and the reason is kept for the
    /// status bar (the host may reopen the file instead).
    fn abandon_base(&mut self, message: impl Into<String>) -> Effect {
        self.base_download = None;
        self.base_request = None;
        self.pending_snapshot = None;
        self.last_error = Some(message.into());
        Effect::Rollback
    }

    /// The base the session assembled from a chain, for the host to install in place of its
    /// replica; at most once per chain. `None` while a chain is still being assembled.
    pub fn take_ready_base(&mut self) -> Option<ReadyBase> {
        self.ready_base.take()
    }

    /// Whether the last snapshot was refused because it did not cover a rewind. The host must skip
    /// that snapshot: its state machine stayed at the rewound version, and installing the answer's
    /// base or tail would advance the replica past the steps it is missing. Reading it clears the
    /// flag, so one refusal is reported once.
    pub fn take_refused_snapshot(&mut self) -> bool {
        std::mem::take(&mut self.snapshot_refused)
    }

    /// Whether a chained base is being assembled right now (the status bar says "rebuilding").
    pub fn rebuilding(&self) -> bool {
        self.pending_snapshot.is_some()
    }

    /// Put the state machine back to the version the replica actually holds and ask for the tail
    /// from there. Used when a local preview is abandoned: the steps the room accepted above this
    /// version are replayed from the tail instead of being skipped as "already seen".
    ///
    /// A resync already on the wire was asked from a version above this one, so its answer will
    /// not carry the steps in between: the rewound version is remembered and that answer is
    /// refused (see the `Snapshot` arm) instead of being applied and clearing the request.
    pub fn rewind_replica(&mut self, version: u64) {
        self.version = version;
        self.replica_version = version;
        if self.resync_in_flight {
            self.rewind_pending = Some(version);
        }
        self.resync_wanted = true;
    }
}

/// Whether `snapshot` carries every step above `version`: either its base replaces the replica at
/// or above that version, or its tail starts at the next one (an empty tail covers a head at or
/// below it).
fn snapshot_covers(snapshot: &SnapshotMsg, version: u64) -> bool {
    if let Some(base) = &snapshot.base
        && base.version >= version
    {
        return true;
    }
    match snapshot.tail.first() {
        Some(first) => first.version <= version.saturating_add(1),
        None => snapshot.version <= version,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_collab_proto::{
        Applied, BaseSnapshot, Capabilities, Delta, Hint, Limits, OpKind, Payload, Rejected, SnapshotKeys, SnapshotMsg, SnapshotRef, Target, Welcome,
    };

    fn op(id: &str, base: u64) -> OpEnvelope {
        OpEnvelope {
            op_id: id.into(),
            doc: "d".into(),
            base_version: base,
            kind: OpKind::Command,
            target: Target::default(),
            payload: Payload::ShapeMove { x: 0.0, y: 0.0 },
            history: None,
            preview: None,
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
    fn a_rewind_during_an_outstanding_resync_keeps_asking_from_the_rewound_version() {
        let mut c = ClientSession::new("d");
        c.on_msg(welcome(0, Role::Editor));
        for n in 1..=4u64 {
            assert_eq!(c.on_msg(applied(&format!("o{n}"), n)), Effect::Settled);
        }
        // A gap asks for the tail from the version the replica holds.
        assert_eq!(c.on_msg(applied("o6", 6)), Effect::Rollback);
        assert!(matches!(c.next_outgoing(), Some(ClientMsg::Resync { last_version: 4 })));
        // The host puts its replica back to 2 while that request is on the wire, so the answer
        // starts at 5 and cannot carry steps 3 and 4.
        c.rewind_replica(2);
        let above = ServerMsg::Snapshot(SnapshotMsg {
            version: 6,
            snapshot: SnapshotRef { mode: SnapshotMode::BaseAndTail, cursor: "tail@6".into() },
            tail: vec![
                Applied { op_id: "o5".into(), version: 5, actor_id: "a".into(), command: None, delta: Delta::default() },
                Applied { op_id: "o6".into(), version: 6, actor_id: "a".into(), command: None, delta: Delta::default() },
            ],
            base: None,
        });
        assert_eq!(c.on_msg(above), Effect::Rollback);
        assert_eq!(c.version(), 2, "the replica stays at the version it was rewound to");
        assert_eq!(c.replica_version, 2, "the snapshot must not advance the replica past the rewind");
        let again = c.next_outgoing().expect("the tail is asked for again from the rewound version");
        assert!(matches!(again, ClientMsg::Resync { last_version: 2 }), "{again:?}");
        // The answer to that request starts at 3, so it covers the rewind and reaches the head.
        let covering = ServerMsg::Snapshot(SnapshotMsg {
            version: 6,
            snapshot: SnapshotRef { mode: SnapshotMode::BaseAndTail, cursor: "tail@6".into() },
            tail: (3..=6)
                .map(|n| Applied { op_id: format!("o{n}"), version: n, actor_id: "a".into(), command: None, delta: Delta::default() })
                .collect(),
            base: None,
        });
        assert_eq!(c.on_msg(covering), Effect::Rollback);
        assert_eq!(c.version(), 6);
        assert!(c.next_outgoing().is_none(), "no further resync is owed");
    }

    #[test]
    fn a_base_at_or_above_the_rewound_version_answers_the_rewind() {
        let mut c = ClientSession::new("d");
        c.on_msg(welcome(0, Role::Editor));
        assert_eq!(c.on_msg(applied("o1", 1)), Effect::Settled);
        assert_eq!(c.on_msg(applied("o3", 3)), Effect::Rollback, "a gap asks for the tail");
        assert!(matches!(c.next_outgoing(), Some(ClientMsg::Resync { last_version: 1 })));
        c.rewind_replica(1);
        // A base at 3 replaces the replica outright: nothing between 1 and 3 is missing.
        let snapshot = ServerMsg::Snapshot(SnapshotMsg {
            version: 3,
            snapshot: SnapshotRef { mode: SnapshotMode::Base, cursor: "base@3".into() },
            tail: vec![],
            base: Some(BaseSnapshot { version: 3, bytes_base64: None, keys: SnapshotKeys::default(), chunks: None }),
        });
        assert_eq!(c.on_msg(snapshot), Effect::Rollback);
        assert_eq!(c.version(), 3, "a base above the rewind covers it");
    }

    #[test]
    fn an_operation_submitted_after_a_resync_waits_for_the_snapshot_and_is_not_dropped_as_covered() {
        let mut c = ClientSession::new("d");
        c.on_msg(welcome(0, Role::Editor));
        c.submit(op("o0", 0)).expect("queue");
        c.next_outgoing().expect("o0 goes out");
        // A gap (a step we never saw) asks for the tail instead of guessing.
        assert_eq!(c.on_msg(applied("o2", 2)), Effect::Rollback);
        assert!(matches!(c.next_outgoing(), Some(ClientMsg::Resync { last_version: 0 })));
        // A new edit while the resync is outstanding: it must not go out yet, or the snapshot
        // that answers the resync would remove it as "covered" although the server never saw it.
        c.submit(op("o1", 0)).expect("queue");
        assert!(c.next_outgoing().is_none(), "nothing is sent while the resync is outstanding");
        let snapshot = ServerMsg::Snapshot(SnapshotMsg {
            version: 2,
            snapshot: SnapshotRef { mode: SnapshotMode::BaseAndTail, cursor: "tail@2".into() },
            tail: vec![],
            base: None,
        });
        c.on_msg(snapshot);
        let sent = c.next_outgoing().expect("the held edit goes after the snapshot");
        assert!(matches!(sent, ClientMsg::Op(ref op) if op.op_id == "o1" && op.base_version == 2), "{sent:?}");
        assert_eq!(c.queued(), 0);
    }

    #[test]
    fn a_gap_during_an_outstanding_resync_never_puts_a_second_request_on_the_wire() {
        let mut c = ClientSession::new("d");
        c.on_msg(welcome(0, Role::Editor));
        c.submit(op("o0", 0)).expect("queue");
        c.next_outgoing().expect("o0 goes out");
        // A gap asks for the tail.
        assert_eq!(c.on_msg(applied("o2", 2)), Effect::Rollback);
        assert!(matches!(c.next_outgoing(), Some(ClientMsg::Resync { last_version: 0 })));
        // A second gap while that request is in flight must not send a second `Resync`: the first
        // snapshot would release the queue, and the second one could then drop an operation the
        // server never applied.
        assert_eq!(c.on_msg(applied("o4", 4)), Effect::Rollback);
        assert!(c.next_outgoing().is_none(), "one resync on the wire at a time");
        c.submit(op("o1", 0)).expect("queue");
        assert!(c.next_outgoing().is_none(), "nothing goes out while the answer is outstanding");
        let snapshot = ServerMsg::Snapshot(SnapshotMsg {
            version: 4,
            snapshot: SnapshotRef { mode: SnapshotMode::BaseAndTail, cursor: "tail@4".into() },
            tail: vec![],
            base: None,
        });
        c.on_msg(snapshot);
        // The snapshot answers the gap it covers, so no second resync is owed, and the held edit
        // goes out against the head it installed.
        let sent = c.next_outgoing().expect("the held edit goes after the snapshot");
        assert!(matches!(sent, ClientMsg::Op(ref op) if op.op_id == "o1" && op.base_version == 4), "{sent:?}");
        assert_eq!(c.in_flight(), 1, "the edit is on the wire, not dropped as covered");
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
            base: Some(BaseSnapshot { version: 6, bytes_base64: Some("UEsD".into()), keys: SnapshotKeys::default(), chunks: None }),
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

    // ------------------------------------------------------------ a base sent as a chain

    /// The bytes of a chained base, as the server would cut them.
    fn base_bytes(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    fn chain_of(bytes: &[u8], chunk_size: u32) -> BaseChunks {
        BaseChunks {
            sha256: crate::media::sha256_hex(bytes),
            len: bytes.len() as u64,
            chunk_size,
            count: bytes.len().div_ceil(chunk_size as usize) as u32,
        }
    }

    fn chained_snapshot(head: u64, base_version: u64, chain: &BaseChunks, tail: Vec<Applied>) -> ServerMsg {
        use deckcraft_collab_proto::{BaseSnapshot, SnapshotKeys};
        ServerMsg::Snapshot(SnapshotMsg {
            version: head,
            snapshot: SnapshotRef { mode: SnapshotMode::Base, cursor: format!("base@{base_version}") },
            tail,
            base: Some(BaseSnapshot { version: base_version, bytes_base64: None, keys: SnapshotKeys::default(), chunks: Some(chain.clone()) }),
        })
    }

    /// A client that joined a room at `head` and was answered with the base at `base_version`
    /// sent as a chain.
    fn chained_client(head: u64, base_version: u64, chain: &BaseChunks, tail: Vec<Applied>) -> ClientSession {
        let mut c = ClientSession::new("d");
        c.on_msg(welcome(head, Role::Editor));
        assert_eq!(c.next_outgoing(), Some(ClientMsg::Resync { last_version: 0 }), "the join asks for the state");
        c.on_msg(chained_snapshot(head, base_version, chain, tail));
        c
    }

    /// One chunk answer, exactly as the server cuts it.
    fn chunk_answer(version: u64, bytes: &[u8], chain: &BaseChunks, index: u32) -> ServerMsg {
        let start = index as usize * chain.chunk_size as usize;
        let end = (start + chain.chunk_size as usize).min(bytes.len());
        ServerMsg::BaseChunk { version, index, count: chain.count, data: base64::engine::general_purpose::STANDARD.encode(&bytes[start..end]) }
    }

    /// Ask for one chunk and feed the answer back.
    fn take_chunk(c: &mut ClientSession, version: u64, bytes: &[u8], chain: &BaseChunks) -> u32 {
        let Some(ClientMsg::BaseChunk { version: asked, index }) = c.next_outgoing() else { panic!("a chunk request") };
        assert_eq!(asked, version);
        assert_eq!(c.next_outgoing(), None, "one chunk request at a time");
        c.on_msg(chunk_answer(version, bytes, chain, index));
        index
    }

    #[test]
    fn a_chained_base_is_assembled_chunk_by_chunk_and_then_handed_to_the_host() {
        let bytes = base_bytes(2_500);
        let chain = chain_of(&bytes, 1024);
        let tail = vec![
            Applied { op_id: "o7".into(), version: 7, actor_id: "a".into(), command: None, delta: Delta::default() },
            Applied { op_id: "o8".into(), version: 8, actor_id: "a".into(), command: None, delta: Delta::default() },
        ];
        let mut c = chained_client(8, 6, &chain, tail.clone());
        assert!(c.rebuilding(), "the snapshot with a chain does not replace the replica yet");
        assert_eq!(c.take_ready_base(), None, "no base before the first chunk");
        assert_eq!(c.submit(op("q1", 0)), Err(SubmitError::Rebuilding), "an edit against the stale replica is refused");

        let mut asked = Vec::new();
        for _ in 0..chain.count {
            asked.push(take_chunk(&mut c, 6, &bytes, &chain));
        }
        assert_eq!(asked, (0..chain.count).collect::<Vec<_>>(), "the chain is asked in order");
        let ready = c.take_ready_base().expect("the assembled base");
        assert_eq!(ready.bytes, bytes);
        let base = ready.snapshot.base.clone().expect("the snapshot travels with its base");
        assert!(!base.is_chained(), "the host gets a plain base");
        assert_eq!(base.version, 6);
        assert_eq!(ready.snapshot.tail, tail, "and the tail above it");
        assert_eq!(ready.snapshot.version, 8, "and the head the snapshot named");
        assert_eq!(c.take_ready_base(), None, "the base is handed over once");
        assert!(!c.rebuilding());
        assert_eq!(c.queued(), 0, "an edit from the old replica is not replayed onto the base");
        assert_eq!(c.submit(op("q2", 0)), Ok(()), "edits are accepted again once the base is installed");
    }

    #[test]
    fn a_chain_interrupted_mid_way_resumes_at_the_missing_chunk() {
        let bytes = base_bytes(2_500);
        let chain = chain_of(&bytes, 1024);
        assert_eq!(chain.count, 3, "three chunks to interrupt");
        let mut c = chained_client(6, 6, &chain, vec![]);
        for _ in 0..2 {
            take_chunk(&mut c, 6, &bytes, &chain);
        }
        // The socket drops with two of the three chunks assembled.
        c.disconnected();
        let _ = c.connect("jwt", ClientInfo { name: "t".into(), ver: "1".into() });
        c.on_msg(welcome(6, Role::Editor));
        let Some(ClientMsg::BaseChunk { version, index }) = c.next_outgoing() else { panic!("the chain resumes") };
        assert_eq!((version, index), (6, 2), "the client asks for the chunk it is missing, not for the first");
        c.on_msg(chunk_answer(6, &bytes, &chain, index));
        let ready = c.take_ready_base().expect("the assembled base");
        assert_eq!(ready.bytes, bytes, "the resumed chain assembles into the whole base");
    }

    #[test]
    fn a_second_snapshot_for_the_same_chain_continues_it_instead_of_restarting() {
        let bytes = base_bytes(2_500);
        let chain = chain_of(&bytes, 1024);
        let mut c = chained_client(6, 6, &chain, vec![]);
        take_chunk(&mut c, 6, &bytes, &chain);
        // The catch-up after a reconnect names the same base again: the prefix is kept.
        c.on_msg(chained_snapshot(
            7,
            6,
            &chain,
            vec![Applied { op_id: "o7".into(), version: 7, actor_id: "a".into(), command: None, delta: Delta::default() }],
        ));
        let Some(ClientMsg::BaseChunk { version, index }) = c.next_outgoing() else { panic!("the chain continues") };
        assert_eq!((version, index), (6, 1), "the assembled prefix is not thrown away");
        assert_eq!(c.take_ready_base(), None);
    }

    #[test]
    fn a_base_the_server_stops_serving_drops_the_partial_chain_and_asks_for_the_session_again() {
        let bytes = base_bytes(2_500);
        let chain = chain_of(&bytes, 1024);
        let mut c = chained_client(6, 6, &chain, vec![]);
        let Some(ClientMsg::BaseChunk { index, .. }) = c.next_outgoing() else { panic!("a chunk request") };
        c.on_msg(ServerMsg::Rejected(Rejected {
            op_id: format!("base@6/{index}"),
            code: RejectCode::Stale,
            message: "the base was replaced by a newer snapshot".into(),
            retryable: true,
            hint: None,
        }));
        assert!(!c.rebuilding(), "the partial chain is dropped");
        assert!(c.last_error().is_some_and(|error| error.contains("replaced")), "{:?}", c.last_error());
        assert_eq!(c.next_outgoing(), Some(ClientMsg::Resync { last_version: 0 }), "a fresh snapshot, from the replica");
        assert_eq!(c.take_ready_base(), None);
    }

    #[test]
    fn a_chain_outside_the_ceilings_is_refused_before_anything_is_allocated() {
        let bytes = base_bytes(2_500);
        let mut c = chained_client(6, 6, &chain_of(&bytes, 1024), vec![]);
        take_chunk(&mut c, 6, &bytes, &chain_of(&bytes, 1024));
        // A depth that does not match len and chunk size: a hostile or broken server.
        let broken = BaseChunks { count: 1, ..chain_of(&bytes, 1024) };
        c.on_msg(chained_snapshot(7, 6, &broken, vec![]));
        assert!(!c.rebuilding(), "a chain outside the ceilings is not assembled");
        assert_eq!(c.last_error().map(str::to_string), Some("the session base is announced as a chain this client cannot accept".into()));
        assert_eq!(c.next_outgoing(), None, "nothing is asked for");
        // A chunk size above the protocol ceiling is refused the same way.
        let too_big =
            BaseChunks { sha256: crate::media::sha256_hex(&bytes), len: bytes.len() as u64, chunk_size: BaseChunks::MAX_CHUNK_SIZE + 1, count: 1 };
        c.on_msg(chained_snapshot(7, 6, &too_big, vec![]));
        assert!(!c.rebuilding());
        assert_eq!(c.take_ready_base(), None);
    }

    #[test]
    fn a_chunk_of_the_wrong_size_or_digest_abandons_the_chain() {
        let bytes = base_bytes(2_500);
        let chain = chain_of(&bytes, 1024);
        let mut c = chained_client(6, 6, &chain, vec![]);
        let Some(ClientMsg::BaseChunk { index, .. }) = c.next_outgoing() else { panic!("a chunk request") };
        // A short chunk is not the one the chain announced.
        c.on_msg(ServerMsg::BaseChunk { version: 6, index, count: chain.count, data: "UEsD".into() });
        assert!(!c.rebuilding(), "a chunk that does not match the chain stops it");
        assert!(c.last_error().is_some_and(|error| error.contains("size")), "{:?}", c.last_error());

        // A whole chain whose bytes do not hash to the announced digest is refused at the end.
        let wrong = BaseChunks { sha256: crate::media::sha256_hex(b"something else"), ..chain };
        c.on_msg(chained_snapshot(6, 6, &wrong, vec![]));
        for _ in 0..wrong.count {
            take_chunk(&mut c, 6, &bytes, &wrong);
        }
        assert_eq!(c.take_ready_base(), None, "a base that does not match its digest is not installed");
        assert!(c.last_error().is_some_and(|error| error.contains("digest")), "{:?}", c.last_error());
    }

    #[test]
    fn steps_that_arrive_while_the_base_assembles_are_collected_by_a_catch_up() {
        let bytes = base_bytes(2_500);
        let chain = chain_of(&bytes, 1024);
        let tail = vec![
            Applied { op_id: "o7".into(), version: 7, actor_id: "a".into(), command: None, delta: Delta::default() },
            Applied { op_id: "o8".into(), version: 8, actor_id: "a".into(), command: None, delta: Delta::default() },
        ];
        let mut c = chained_client(8, 6, &chain, tail);
        take_chunk(&mut c, 6, &bytes, &chain);
        // Someone else's step arrives while the base is on its way: it must not move the replica.
        assert_eq!(c.on_msg(applied("o9", 9)), Effect::None);
        assert_eq!(c.version(), 8, "the head does not move on a step the replica cannot apply");
        for _ in 1..chain.count {
            take_chunk(&mut c, 6, &bytes, &chain);
        }
        let ready = c.take_ready_base().expect("the assembled base");
        assert_eq!(ready.snapshot.version, 8);
        assert_eq!(c.next_outgoing(), Some(ClientMsg::Resync { last_version: 8 }), "the missed step is asked for from the base");
    }

    // ------------------------------------------------------------ a Save becomes a commit

    fn commit_result(version: u64, file_version: Option<&str>) -> ServerMsg {
        ServerMsg::CommitResult(deckcraft_collab_proto::CommitResult {
            sha256: "ab".repeat(32),
            version,
            file_version: file_version.map(str::to_string),
            bytes_base64: None,
            blob: None,
            warnings: vec![],
        })
    }

    #[test]
    fn a_commit_waits_for_the_journal_and_travels_once() {
        let mut c = ClientSession::new("d");
        c.on_msg(welcome(0, Role::Editor));
        c.submit(op("o1", 0)).expect("queue");
        c.commit(Some("etag-0".into())).expect("commit");
        assert!(matches!(c.next_outgoing(), Some(ClientMsg::Op(_))), "the edit goes first");
        assert_eq!(c.next_outgoing(), None, "the commit waits for the answer to the edit");
        c.on_msg(applied("o1", 1));
        assert_eq!(c.next_outgoing(), Some(ClientMsg::Commit { expected_file_version: Some("etag-0".into()) }));
        assert_eq!(c.next_outgoing(), None);
        // A second Save while the commit is on its way is the same request.
        c.commit(Some("etag-0".into())).expect("deduped");
        assert_eq!(c.next_outgoing(), None, "one commit in flight, not two");
        assert_eq!(c.on_msg(commit_result(1, Some("etag-1"))), Effect::None);
        assert!(!c.commit_in_flight());
        assert_eq!(c.file_version(), Some("etag-1"));
        // The next Save expects the version the cabinet accepted.
        c.commit(c.file_version().map(str::to_string)).expect("second");
        assert_eq!(c.next_outgoing(), Some(ClientMsg::Commit { expected_file_version: Some("etag-1".into()) }));
    }

    #[test]
    fn a_commit_with_an_empty_journal_is_still_allowed() {
        let mut c = ClientSession::new("d");
        c.on_msg(welcome(0, Role::Editor));
        c.commit(None).expect("commit");
        assert_eq!(c.next_outgoing(), Some(ClientMsg::Commit { expected_file_version: None }), "a version is created from unchanged bytes");
    }

    #[test]
    fn a_viewer_and_an_offline_session_are_refused_before_anything_is_sent() {
        let mut c = ClientSession::new("d");
        assert_eq!(c.commit(None), Err(CommitError::Offline), "no handshake yet");
        c.on_msg(welcome(0, Role::Viewer));
        assert_eq!(c.commit(None), Err(CommitError::ReadOnly));
        assert_eq!(c.next_outgoing(), None, "the server is never asked");
    }

    #[test]
    fn a_refused_commit_is_not_retried_by_itself_and_keeps_its_expected_version() {
        let mut c = ClientSession::new("d");
        c.on_msg(welcome(0, Role::Editor));
        c.commit(Some("etag-0".into())).expect("commit");
        assert!(matches!(c.next_outgoing(), Some(ClientMsg::Commit { .. })));
        c.on_msg(ServerMsg::Rejected(Rejected {
            op_id: "commit".into(),
            code: RejectCode::Conflict,
            message: "the file changed outside the session".into(),
            retryable: false,
            hint: None,
        }));
        assert!(!c.commit_in_flight(), "the refusal frees the request");
        assert_eq!(c.next_outgoing(), None, "a refusal is not retried by itself");
        assert_eq!(c.file_version(), None, "nothing was accepted");
        // The user may press Save again; the expected version is still the old one, so the
        // server refuses again instead of overwriting the version that moved on.
        c.commit(Some("etag-0".into())).expect("retry");
        assert_eq!(c.next_outgoing(), Some(ClientMsg::Commit { expected_file_version: Some("etag-0".into()) }));
    }
}
