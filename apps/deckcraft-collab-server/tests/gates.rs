//! The gates TASK.md §8 asks for beyond the command slices: a 100+ slide deck with media stays
//! fast through two clients and a commit, the `.pptx` a commit produces opens in LibreOffice
//! without repair and survives a LibreOffice round trip, and three real sessions (two editors and
//! a viewer) converge across a conflicting edit and a reconnect with a journal catch-up.
//!
//! The performance numbers are printed (`--nocapture`); the assertions keep generous ceilings, so
//! the gate catches a pathological regression rather than machine-to-machine noise. The
//! LibreOffice test skips itself, with a message, when `soffice` is not installed.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine as _;
use deckcraft_collab::{Adapter, ClientSession, DocSession, ReadyBase, SessionConfig};
use deckcraft_collab_proto::{
    Applied, BaseSnapshot, Capabilities, ClientInfo, ClientMsg, FillSpec, Limits, OpEnvelope, OpKind, PROTO_VERSION, Payload, Role, ServerMsg,
    ShapeKey, SlideKey, SnapshotMode, SnapshotMsg, SnapshotRef, Target, Welcome,
};
use deckcraft_engine::Session;
use deckcraft_model::{Presentation, ShapeKind};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{Value, json};
use tungstenite::stream::MaybeTlsStream;

type Client = tungstenite::WebSocket<MaybeTlsStream<TcpStream>>;

const SECRET: &[u8] = b"test-secret-for-deckcraft-collab";
const SLIDES: usize = 120;
const PICTURES: usize = 4;

// ---------------------------------------------------------------- the large deck

/// One 64×64 PNG per picture object; four distinct objects, reused across slides.
fn swatch_png(n: usize) -> Vec<u8> {
    let mut pixels = vec![0u8; 64 * 64 * 4];
    for (i, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        px[0] = ((i % 64) as u32 * 3 + n as u32 * 40).min(255) as u8;
        px[1] = (i / 64) as u8 * 3 + (n as u8) * 20;
        px[2] = (n as u8) * 60;
        px[3] = 255;
    }
    deckcraft_render::Image { width: 64, height: 64, pixels }.to_png()
}

/// `SLIDES` slides, each with a picture and a rectangle. The engine builds it the same way the UI
/// would, so the media bytes live in the presentation.
fn large_deck() -> (Presentation, f64) {
    let started = Instant::now();
    let pictures: Vec<Vec<u8>> = (0..PICTURES).map(swatch_png).collect();
    let mut session = Session::with_new();
    for i in 0..SLIDES {
        if i > 0 {
            session.apply_remote("slide.new", &json!({"layout": "blank", "at": i})).expect("slide");
        }
        let data = base64::engine::general_purpose::STANDARD.encode(&pictures[i % PICTURES]);
        session
            .apply_remote(
                "insert.picture",
                &json!({"name": format!("swatch-{}.png", i % PICTURES), "data": data, "rect": [40.0, 60.0, 480.0, 270.0]}),
            )
            .expect("picture");
        session.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [560.0, 80.0, 300.0, 120.0]})).expect("rect");
    }
    let doc = session.doc().expect("doc").doc.as_ref().clone();
    (doc, started.elapsed().as_secs_f64() * 1000.0)
}

fn picture_count(doc: &Presentation) -> usize {
    doc.slides.iter().flat_map(|slide| slide.shapes.iter()).filter(|shape| matches!(shape.kind, ShapeKind::Picture { .. })).count()
}

// ---------------------------------------------------------------- two clients, no socket

struct Peer {
    client: ClientSession,
    actor: String,
}

fn welcome(version: u64, actor: &str) -> ServerMsg {
    ServerMsg::Welcome(Welcome {
        v: PROTO_VERSION,
        session_id: "gate".into(),
        actor_id: actor.into(),
        role: Role::Editor,
        lease: None,
        version,
        snapshot: SnapshotRef { mode: SnapshotMode::BaseAndTail, cursor: format!("tail@{version}") },
        capabilities: Capabilities {
            kinds: vec![OpKind::Command, OpKind::Structural],
            limits: Limits::default(),
            commands: vec!["shape.move".into()],
        },
    })
}

fn join(server: &DocSession, actor: &str) -> Peer {
    let mut client = ClientSession::new(server.file_id());
    assert!(matches!(client.connect("jwt", ClientInfo { name: "gate".into(), ver: "0.1".into() }), ClientMsg::Hello(_)));
    client.on_msg(welcome(server.version(), actor));
    Peer { client, actor: actor.into() }
}

/// Move the peer's queued messages through the server and fan the answer out to everyone.
fn pump(server: &mut DocSession, peer: &mut Peer, others: &mut [&mut Peer]) {
    for _ in 0..10 {
        let Some(msg) = peer.client.next_outgoing() else { return };
        match msg {
            ClientMsg::Op(op) => match server.apply(&peer.actor, &op) {
                Ok(applied) => {
                    peer.client.on_msg(ServerMsg::Applied(applied.clone()));
                    for other in others.iter_mut() {
                        other.client.on_msg(ServerMsg::Applied(applied.clone()));
                    }
                }
                Err(rejected) => {
                    peer.client.on_msg(ServerMsg::Rejected(rejected));
                }
            },
            ClientMsg::Resync { last_version } => {
                let snapshot = server.resync(last_version);
                peer.client.on_msg(ServerMsg::Snapshot(snapshot));
            }
            _ => return,
        }
    }
}

/// The slide and its picture, as wire keys.
fn picture_key(server: &DocSession, index: usize) -> (SlideKey, ShapeKey) {
    let doc = server.document().expect("doc");
    let slide = doc.slides.get(index).expect("slide");
    let shape = slide.shapes.iter().find(|shape| matches!(shape.kind, ShapeKind::Picture { .. })).expect("picture");
    let keys = server.keys();
    (keys.slide_key(slide.id).expect("slide key").clone(), keys.shape_key(slide.id, shape.id).expect("shape key").clone())
}

fn move_op(index: usize, slide: &SlideKey, shape: &ShapeKey, x: f64) -> OpEnvelope {
    OpEnvelope {
        op_id: format!("move-{index}"),
        doc: "gate".into(),
        base_version: 0,
        kind: OpKind::Command,
        target: Target { slide: Some(slide.clone()), shape: Some(shape.clone()), ..Target::default() },
        payload: Payload::ShapeMove { x, y: 60.0 },
        history: None,
        preview: None,
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let at = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted.get(at).copied().unwrap_or(0.0)
}

fn summarize(name: &str, mut ms: Vec<f64>) -> f64 {
    ms.sort_by(f64::total_cmp);
    let p50 = percentile(&ms, 0.50);
    let p95 = percentile(&ms, 0.95);
    let max = ms.last().copied().unwrap_or(0.0);
    eprintln!("deckcraft gate: {name}: {p50:.1} ms p50, {p95:.1} ms p95, {max:.1} ms max over {} steps", ms.len());
    max
}

// ---------------------------------------------------------------- performance gate

#[test]
fn a_hundred_and_twenty_slide_deck_with_media_stays_fast_and_converges() {
    let (doc, build_ms) = large_deck();
    assert_eq!(doc.slides.len(), SLIDES);
    assert_eq!(picture_count(&doc), SLIDES, "every slide carries a picture");
    eprintln!("deckcraft gate: built {SLIDES} slides with {} media objects in {build_ms:.0} ms", doc.media.len());

    let opened = Instant::now();
    let mut server = DocSession::from_document("gate", doc, SessionConfig::default()).expect("session");
    let open_ms = opened.elapsed().as_secs_f64() * 1000.0;
    eprintln!("deckcraft gate: opened the room in {open_ms:.0} ms ({} objects on the upload channel)", server.media_entries().len());

    let mut a = join(&server, "alice");
    let mut b = join(&server, "bob");
    let mut apply_ms = Vec::with_capacity(SLIDES);
    for index in 0..SLIDES {
        let (slide, shape) = picture_key(&server, index);
        let op = move_op(index, &slide, &shape, 80.0);
        a.client.submit(op).expect("submit");
        let started = Instant::now();
        pump(&mut server, &mut a, &mut [&mut b]);
        apply_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(
            server.version(),
            index as u64 + 1,
            "one authoritative step per move (author {}, other {}): {:?}",
            a.client.version(),
            b.client.version(),
            a.client.last_error()
        );
    }
    assert_eq!(a.client.version(), SLIDES as u64, "the author settled");
    assert_eq!(b.client.version(), SLIDES as u64, "the other editor settled");
    let max_apply = summarize("apply shape.move", apply_ms);
    assert!(max_apply < 2_000.0, "one authoritative move took {max_apply:.0} ms on {SLIDES} slides");

    let xs: Vec<f64> = server
        .document()
        .expect("doc")
        .slides
        .iter()
        .flat_map(|slide| slide.shapes.iter())
        .filter(|shape| matches!(shape.kind, ShapeKind::Picture { .. }))
        .map(|shape| shape.xfrm.expect("picture transform").x)
        .collect();
    assert_eq!(xs.len(), SLIDES, "every slide still has its picture");
    assert!(xs.iter().all(|x| (*x - 80.0).abs() < 0.001), "every picture moved on the authoritative document");

    let started = Instant::now();
    let committed = server.commit_checked(None).expect("commit");
    let commit_ms = started.elapsed().as_secs_f64() * 1000.0;
    eprintln!("deckcraft gate: commit built {} KiB in {commit_ms:.0} ms", committed.bytes.len() / 1024);
    assert!(committed.bytes.starts_with(b"PK"), "a .pptx is a zip");
    assert!(commit_ms < 60_000.0, "commit took {commit_ms:.0} ms");
    assert!(committed.warnings.is_empty(), "{:?}", committed.warnings);

    let reopened = deckcraft_pptx::import(&committed.bytes).expect("the committed bytes reimport");
    assert_eq!(reopened.slides.len(), SLIDES);
    assert_eq!(picture_count(&reopened), SLIDES, "media survived the file");

    let again = Instant::now();
    let cached = server.commit_checked(None).expect("second commit");
    eprintln!("deckcraft gate: an unchanged commit answered in {:.1} ms", again.elapsed().as_secs_f64() * 1000.0);
    assert_eq!(cached.bytes, committed.bytes, "an unchanged commit reuses the built file (ADR-0009 §6)");
}

// ---------------------------------------------------------------- three sessions gate

/// A local replica: the initial document plus every confirmed command the server broadcasts.
/// The gate checks convergence on real presentation state, not only on version counters.
struct Replica {
    session: Session,
    adapter: Adapter,
    version: u64,
    error: Option<String>,
}

impl Replica {
    fn open(bytes: &[u8], file_id: &str) -> Self {
        let session = replica_session(bytes);
        let doc = session.doc().expect("replica doc").doc.as_ref().clone();
        let mut adapter = Adapter::new(format!("{file_id}-"));
        adapter.join(&doc);
        Self { session, adapter, version: 0, error: None }
    }

    /// Replace the replica with a base the server sent after compaction and take the key map it
    /// carries, so the operations above it address the same objects as on every other replica.
    fn rebuild(&mut self, bytes: &[u8], base: &BaseSnapshot) -> Result<(), String> {
        let session = replica_session(bytes);
        self.adapter.restore_keys(&base.keys);
        self.session = session;
        self.version = base.version;
        self.error = None;
        Ok(())
    }

    /// Run the command the server confirmed, at the version the replica has.
    fn apply(&mut self, applied: &Applied) {
        if let Some(command) = &applied.command {
            let op = OpEnvelope {
                op_id: applied.op_id.clone(),
                doc: String::new(),
                base_version: self.version,
                kind: deckcraft_collab::class_of(&command.payload),
                target: command.target.clone(),
                payload: command.payload.clone(),
                history: None,
                preview: None,
            };
            if let Err(rejection) = self.adapter.apply_confirmed(&mut self.session, self.version, &applied.actor_id, &op) {
                self.error = Some(rejection.message.clone());
            }
        }
        self.version = self.version.max(applied.version);
    }

    /// Reconnect catch-up: replay the journal tail after the version the replica has.
    fn catch_up(&mut self, snapshot: &SnapshotMsg) {
        for applied in &snapshot.tail {
            self.apply(applied);
        }
        self.version = self.version.max(snapshot.version);
    }

    fn doc(&self) -> &Presentation {
        self.session.doc().expect("replica doc").doc.as_ref()
    }
}

/// A session over the bytes every participant opened, ready for remote commands.
fn replica_session(bytes: &[u8]) -> Session {
    let mut session = Session::new();
    let loaded = deckcraft_engine::cmd::file::loaded_file("replica.deck", bytes).expect("load the replica deck");
    let token = session.stage_loaded(loaded);
    session.execute("file.adopt", &json!({"path": "replica.deck", "token": token})).expect("adopt the replica deck");
    session
}

/// One WebSocket participant: the protocol state plus the replica the gate compares.
struct Live {
    ws: Option<Client>,
    client: ClientSession,
    replica: Replica,
    actor: String,
    /// The base the last `resync` delivered: its version and its decoded size in bytes.
    last_base: Option<(u64, usize)>,
    /// How many journal steps the last `resync` delivered.
    last_tail: usize,
    /// How many chunks the last chained base arrived in (`None` when it came inline).
    last_chunks: Option<usize>,
    /// The chunk the client asked for last, so a resumed chain can be checked.
    last_chunk_request: Option<u32>,
}

impl Live {
    fn join(addr: SocketAddr, id: &str, actor: &str, role: &str, bytes: &[u8]) -> Self {
        let mut live = Self::connect(addr, id, actor, role, bytes);
        live.catch_up_after_join();
        live
    }

    /// A connected participant that has not caught up yet: the caller pumps what it asks for, so a
    /// gate can drop the socket half-way through a chained base.
    fn connect(addr: SocketAddr, id: &str, actor: &str, role: &str, bytes: &[u8]) -> Self {
        let (mut ws, _) = tungstenite::connect(format!("ws://{addr}/v1/sessions/{id}/ws")).expect("ws");
        socket_timeout(&mut ws);
        let mut client = ClientSession::new(id);
        let hello = client.connect(token(actor, role, id), ClientInfo { name: actor.into(), ver: "1".into() });
        ws.send(tungstenite::Message::Text(deckcraft_collab_proto::encode_client(&hello).expect("encode").into())).expect("hello");
        let welcome = read_socket(&mut ws);
        let mut live = Self {
            ws: Some(ws),
            client,
            replica: Replica::open(bytes, id),
            actor: actor.into(),
            last_base: None,
            last_tail: 0,
            last_chunks: None,
            last_chunk_request: None,
        };
        live.on(welcome);
        live
    }

    /// A client that joins a room that already moved asks for the state before it reports
    /// anything else: the journal tail, or the base plus the tail when compaction removed it. A
    /// base above the inline limit arrives as a chain, so the catch-up also asks for its chunks.
    fn catch_up_after_join(&mut self) {
        self.pump_catch_up();
    }

    /// The replica the session assembled from a chain, installed exactly as the desktop host
    /// installs it: replace the document, take the key map, replay the tail above the base.
    fn install_ready_base(&mut self, ready: ReadyBase) {
        let base = ready.snapshot.base.clone().expect("an assembled base carries its snapshot");
        self.replica.rebuild(&ready.bytes, &base).expect("the assembled base reloads");
        self.last_base = Some((base.version, ready.bytes.len()));
        self.last_tail = ready.snapshot.tail.len();
        self.replica.catch_up(&ready.snapshot);
    }

    fn on(&mut self, msg: ServerMsg) {
        let mut deferred = false;
        match &msg {
            ServerMsg::Applied(applied) => self.replica.apply(applied),
            ServerMsg::Snapshot(snapshot) => {
                self.last_tail = snapshot.tail.len();
                self.last_base = None;
                self.last_chunks = None;
                match &snapshot.base {
                    Some(base) if base.is_chained() => {
                        // The base travels in chunks: the client state machine assembles it and the
                        // host installs it once it is whole, exactly like the desktop app does.
                        let chain = base.chunks.as_ref().expect("a chained base names its chain");
                        self.last_chunks = Some(chain.count as usize);
                        deferred = true;
                    }
                    Some(base) => {
                        let encoded = base.bytes_base64.as_deref().expect("the gate's inline base carries bytes");
                        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).expect("base64");
                        self.last_base = Some((base.version, bytes.len()));
                        self.replica.rebuild(&bytes, base).expect("the base reloads");
                    }
                    None => {}
                }
                if !deferred {
                    self.replica.catch_up(snapshot);
                }
            }
            _ => {}
        }
        self.client.on_msg(msg);
        if let Some(ready) = self.client.take_ready_base() {
            self.install_ready_base(ready);
        }
    }

    fn send(&mut self, msg: ClientMsg) {
        let ws = self.ws.as_mut().expect("a live socket");
        ws.send(tungstenite::Message::Text(deckcraft_collab_proto::encode_client(&msg).expect("encode").into())).expect("send");
    }

    fn recv(&mut self) -> ServerMsg {
        let msg = read_socket(self.ws.as_mut().expect("a live socket"));
        self.on(msg.clone());
        msg
    }

    fn submit(&mut self, op: OpEnvelope) {
        self.client.submit(op).expect("submit");
        let msg = self.client.next_outgoing().expect("a queued operation");
        self.send(msg);
    }

    /// The socket drops; the client keeps its unacknowledged work, the replica its document.
    fn hang_up(&mut self) {
        if let Some(mut ws) = self.ws.take() {
            let _ = ws.close(None);
        }
        self.client.disconnected();
        // The server removes the old peer after its read loop notices the close. A reconnect that
        // races that cleanup would be removed by it, so let the loop's 50 ms time slice finish.
        std::thread::sleep(Duration::from_millis(300));
    }

    /// A fresh socket and a hello that names the version the replica has; nothing is pumped yet,
    /// so a caller can see what the client asks for first (a tail, or the chunk it is missing).
    fn reopen(&mut self, addr: SocketAddr, id: &str, role: &str) {
        self.hang_up();
        let (mut ws, _) = tungstenite::connect(format!("ws://{addr}/v1/sessions/{id}/ws")).expect("reconnect ws");
        socket_timeout(&mut ws);
        let hello = self.client.connect(token(&self.actor, role, id), ClientInfo { name: self.actor.clone(), ver: "1".into() });
        ws.send(tungstenite::Message::Text(deckcraft_collab_proto::encode_client(&hello).expect("encode").into())).expect("reconnect hello");
        let welcome = read_socket(&mut ws);
        self.ws = Some(ws);
        self.on(welcome);
    }

    /// One catch-up message: the resync the client asked for, or one chunk of a chained base.
    /// `false` when the client has nothing left to ask for.
    fn pump_one(&mut self) -> bool {
        let Some(msg) = self.client.next_outgoing() else { return false };
        match msg {
            ClientMsg::Resync { .. } => {
                self.send(msg);
                let snapshot = self.recv();
                assert!(matches!(snapshot, ServerMsg::Snapshot(_)), "the catch-up sent {snapshot:?}, not the state");
            }
            ClientMsg::BaseChunk { version, index } => {
                self.last_chunk_request = Some(index);
                self.send(ClientMsg::BaseChunk { version, index });
                let answer = self.recv();
                assert!(matches!(answer, ServerMsg::BaseChunk { .. }), "the catch-up sent {answer:?}, not a chunk");
            }
            other => panic!("a catching-up client sent {other:?}"),
        }
        true
    }

    fn pump_catch_up(&mut self) {
        while self.pump_one() {}
    }

    /// A fresh socket, a hello that names the version the replica has, and the catch-up: the
    /// journal tail, or the rest of a chained base the replica already started.
    fn reconnect(&mut self, addr: SocketAddr, id: &str, role: &str) {
        self.reopen(addr, id, role);
        self.pump_catch_up();
    }
}

fn socket_timeout(ws: &mut Client) {
    if let MaybeTlsStream::Plain(stream) = ws.get_mut() {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
    }
}

/// Two slides, one rectangle each: editors can work on different slides, and two of them can aim
/// at the same shape on the first one.
fn three_way_deck() -> Vec<u8> {
    let mut session = Session::with_new();
    session.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [100.0, 100.0, 200.0, 100.0]})).expect("first rect");
    session.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("second slide");
    session.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [300.0, 100.0, 200.0, 100.0]})).expect("second rect");
    deckcraft_format::save(session.doc().expect("doc").doc.as_ref()).expect("save the gate deck")
}

/// The wire target of the first shape on `slide_index`, from a replica that shares the server's
/// key map (same prefix, same document).
fn rect_target(replica: &Replica, slide_index: usize) -> Target {
    let doc = replica.doc();
    let slide = doc.slides.get(slide_index).expect("slide");
    // The title slide also carries layout placeholders without an explicit transform; the rect
    // this gate moves is the shape that has one.
    let shape = slide.shapes.iter().find(|shape| shape.xfrm.is_some()).expect("shape");
    let keys = replica.adapter.keys();
    Target {
        slide: Some(keys.slide_key(slide.id).expect("slide key").clone()),
        shape: Some(keys.shape_key(slide.id, shape.id).expect("shape key").clone()),
        ..Target::default()
    }
}

fn move_to(op_id: &str, doc: &str, target: Target, x: f64) -> OpEnvelope {
    OpEnvelope {
        op_id: op_id.into(),
        doc: doc.into(),
        base_version: 0,
        kind: OpKind::Command,
        target,
        payload: Payload::ShapeMove { x, y: 100.0 },
        history: None,
        preview: None,
    }
}

fn rect_x(doc: &Presentation, slide_index: usize) -> f64 {
    doc.slides.get(slide_index).expect("slide").shapes.iter().find(|shape| shape.xfrm.is_some()).expect("shape").xfrm.expect("transform").x
}

fn expect_applied(live: &mut Live, version: u64) {
    match live.recv() {
        ServerMsg::Applied(applied) => assert_eq!(applied.version, version),
        other => panic!("expected applied {version}, got {other:?}"),
    }
}

/// Every connected session receives the same authoritative step.
fn step<const N: usize>(peers: [&mut Live; N], version: u64) {
    for live in peers {
        expect_applied(live, version);
    }
}

fn assert_rect_x(peers: [&Live; 3], slide_index: usize, x: f64, why: &str) {
    for live in peers {
        assert_eq!(rect_x(live.replica.doc(), slide_index), x, "{why} ({} replica)", live.actor);
    }
}

/// Alice moves the first slide's rectangle; Bob moves the same shape before seeing that step;
/// Alice's undo is refused because it would overwrite Bob's later value.
fn conflicting_edits_of_one_shape(alice: &mut Live, bob: &mut Live, viewer: &mut Live, id: &str, first: &Target) {
    alice.submit(move_to("a-1", id, first.clone(), 200.0));
    step([&mut *alice, &mut *bob, &mut *viewer], 1);

    // The step is absolute, so the outcome is deterministic: the later accepted value wins.
    bob.submit(move_to("b-1", id, first.clone(), 300.0));
    step([&mut *alice, &mut *bob, &mut *viewer], 2);
    assert_rect_x([&*alice, &*bob, &*viewer], 0, 300.0, "the deterministic outcome is Bob's later value");

    // The server refuses the undo without touching the document (ADR-0010 §3).
    alice.send(ClientMsg::Undo);
    match alice.recv() {
        ServerMsg::Rejected(rejection) => {
            assert_eq!(rejection.code, deckcraft_collab_proto::RejectCode::UndoConflict, "{}", rejection.message);
            assert!(!rejection.retryable);
        }
        other => panic!("expected an undo-conflict, got {other:?}"),
    }
    assert_eq!(alice.replica.version, 2, "a refusal does not move the version");
    assert_rect_x([&*alice, &*bob, &*viewer], 0, 300.0, "the refused undo left Bob's value in place");
}

fn a_viewer_cannot_write(viewer: &mut Live, alice: &Live, id: &str, version: u64) {
    viewer.send(ClientMsg::Op(move_to("v-1", id, rect_target(&alice.replica, 0), 999.0)));
    match viewer.recv() {
        ServerMsg::Rejected(rejection) => {
            assert_eq!(rejection.code, deckcraft_collab_proto::RejectCode::Forbidden);
            assert_eq!(rejection.op_id, "v-1");
        }
        other => panic!("expected the viewer to be refused, got {other:?}"),
    }
    assert_eq!(viewer.replica.version, version, "a forbidden operation does not move the replica");
}

fn assert_converged(peers: [&Live; 3], version: u64) {
    for live in peers {
        assert_eq!(live.client.version(), version, "{} settled at the session head", live.actor);
        assert_eq!(live.replica.version, version);
        assert_eq!(live.replica.error, None, "{} applied every confirmed command", live.actor);
    }
    assert_eq!(peers[0].replica.doc(), peers[1].replica.doc(), "the editors hold one document");
    assert_eq!(peers[0].replica.doc(), peers[2].replica.doc(), "the viewer holds the same document");
}

fn assert_committed_gate_deck(addr: SocketAddr, id: &str, version: u64, first_x: f64, second_x: f64) {
    let (status, body) = http(addr, "POST", &format!("/v1/sessions/{id}/commit"), Some("service-token"), "");
    assert_eq!(status, 200, "{body}");
    let answer: Value = serde_json::from_str(&body).expect("json");
    assert_eq!(answer["version"].as_u64(), Some(version));
    let committed = base64::engine::general_purpose::STANDARD.decode(answer["bytes"].as_str().expect("inline bytes")).expect("decode");
    let reopened = deckcraft_pptx::import(&committed).expect("the committed bytes reimport");
    assert_eq!(reopened.slides.len(), 2);
    assert_eq!(rect_x(&reopened, 0), first_x);
    assert_eq!(rect_x(&reopened, 1), second_x);
}

#[test]
fn three_sessions_converge_across_a_conflict_and_a_reconnect() {
    const ID: &str = "gate-three";
    let bytes = three_way_deck();
    let handle = deckcraft_collab_server::serve_ephemeral(deckcraft_collab_server::Config::for_tests(SECRET, "service-token")).expect("server");
    let addr = handle.addr;
    create(addr, ID, &bytes);

    let mut alice = Live::join(addr, ID, "alice", "editor", &bytes);
    let mut bob = Live::join(addr, ID, "bob", "editor", &bytes);
    let mut viewer = Live::join(addr, ID, "victor", "viewer", &bytes);
    assert_eq!(viewer.client.role(), Some(Role::Viewer));

    let first = rect_target(&alice.replica, 0);
    conflicting_edits_of_one_shape(&mut alice, &mut bob, &mut viewer, ID, &first);

    // Bob works on the second slide (the editors are not forced onto one slide), version 3.
    let second = rect_target(&alice.replica, 1);
    bob.submit(move_to("b-2", ID, second.clone(), 120.0));
    step([&mut alice, &mut bob, &mut viewer], 3);

    // Bob drops off; Alice keeps editing the second slide while he is away.
    bob.hang_up();
    alice.submit(move_to("a-2", ID, second, 150.0));
    step([&mut alice, &mut viewer], 4);

    a_viewer_cannot_write(&mut viewer, &alice, ID, 4);

    // Bob reconnects with the version his replica has (3); the tail brings him to 4.
    bob.reconnect(addr, ID, "editor");
    assert_eq!(bob.replica.version, 4, "the journal tail caught the replica up");
    assert_eq!(rect_x(bob.replica.doc(), 1), 150.0, "the missed edit is in the caught-up document");

    // All three settle on one version and one document; the viewer only read.
    assert_converged([&alice, &bob, &viewer], 4);
    assert_committed_gate_deck(addr, ID, 4, 300.0, 150.0);
    handle.shutdown();
}

// ---------------------------------------------------------------- three-client soak

/// Two slides with one rectangle each; the first one starts with text so `text.set` and
/// `shape.fill` have an object to change from the first step.
fn soak_deck() -> Vec<u8> {
    let mut session = Session::with_new();
    session.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100], "text": "alpha"})).expect("first rect");
    session.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("second slide");
    session.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [300, 100, 200, 100], "text": "beta"})).expect("second rect");
    deckcraft_format::save(session.doc().expect("doc").doc.as_ref()).expect("save the soak deck")
}

fn bounds_to(op_id: &str, doc: &str, target: Target, x: f64, y: f64, w: f64, h: f64) -> OpEnvelope {
    OpEnvelope {
        op_id: op_id.into(),
        doc: doc.into(),
        base_version: 0,
        kind: OpKind::Command,
        target,
        payload: Payload::ShapeSetBounds { x, y, w, h },
        history: None,
        preview: None,
    }
}

fn text_to(op_id: &str, doc: &str, target: Target, text: &str) -> OpEnvelope {
    OpEnvelope {
        op_id: op_id.into(),
        doc: doc.into(),
        base_version: 0,
        kind: OpKind::Command,
        target,
        payload: Payload::TextSet { text: text.into(), cell: None },
        history: None,
        preview: None,
    }
}

fn fill_to(op_id: &str, doc: &str, target: Target, color: &str) -> OpEnvelope {
    OpEnvelope {
        op_id: op_id.into(),
        doc: doc.into(),
        base_version: 0,
        kind: OpKind::Command,
        target,
        payload: Payload::ShapeFill { fill: FillSpec::Solid { color: color.into() } },
        history: None,
        preview: None,
    }
}

/// Two editors make 20+ edits of every merge kind the wire carries (move, bounds, text, fill)
/// while a snapshot every five steps compacts the journal. A viewer then joins: its version is
/// below the base, so the server sends the native base and the key map instead of a tail. One
/// editor drops off, edits continue, and he reconnects from the journal. All three replicas end
/// on one version and one `Presentation`, and the commit at that version reimports with the
/// final values.
#[test]
fn three_clients_soak_across_compaction_and_a_base_transfer() {
    const ID: &str = "gate-soak";
    const SNAPSHOT_EVERY: u64 = 5;
    const ROUNDS: usize = 6;
    let bytes = soak_deck();
    let mut config = deckcraft_collab_server::Config::for_tests(SECRET, "service-token");
    config.snapshot_every = SNAPSHOT_EVERY;
    let handle = deckcraft_collab_server::serve_ephemeral(config).expect("server");
    let addr = handle.addr;
    create(addr, ID, &bytes);

    let mut alice = Live::join(addr, ID, "alice", "editor", &bytes);
    let mut bob = Live::join(addr, ID, "bob", "editor", &bytes);
    let first = rect_target(&alice.replica, 0);
    let second = rect_target(&alice.replica, 1);

    let soak = Instant::now();
    let mut version = 0u64;
    for round in 0..ROUNDS {
        version += 1;
        alice.submit(move_to(&format!("soak-m{round}"), ID, first.clone(), 120.0 + round as f64 * 10.0));
        step([&mut alice, &mut bob], version);

        version += 1;
        bob.submit(bounds_to(&format!("soak-b{round}"), ID, second.clone(), 300.0 + round as f64 * 10.0, 100.0, 200.0 + round as f64, 100.0));
        step([&mut alice, &mut bob], version);

        version += 1;
        alice.submit(text_to(&format!("soak-t{round}"), ID, first.clone(), &format!("alpha r{round}")));
        step([&mut alice, &mut bob], version);

        version += 1;
        let color = format!("#{:02x}{:02x}40", 0x20 + round as u8 * 0x20, 0xff - round as u8 * 0x20);
        bob.submit(fill_to(&format!("soak-f{round}"), ID, first.clone(), &color));
        step([&mut alice, &mut bob], version);
    }
    // 25 is a snapshot boundary (the journal is compacted there); 26 stays in the tail.
    version += 1;
    alice.submit(move_to("soak-m6", ID, first.clone(), 400.0));
    step([&mut alice, &mut bob], version);
    version += 1;
    bob.submit(text_to("soak-t6", ID, first.clone(), "final"));
    step([&mut alice, &mut bob], version);
    assert_eq!(version, 26, "twenty-six edits, all four merge kinds");
    eprintln!(
        "deckcraft gate: {version} edits by two editors in {:.2} s; snapshots every {SNAPSHOT_EVERY} steps compacted the journal",
        soak.elapsed().as_secs_f64()
    );

    // The third client joins below the base: the server sends the native base, its key map and
    // the steps above it.
    let joined = Instant::now();
    let mut viewer = Live::join(addr, ID, "victor", "viewer", &bytes);
    let join_ms = joined.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(viewer.client.version(), 26);
    let (base_version, base_bytes) = viewer.last_base.expect("the viewer received a base");
    assert_eq!(base_version, 25, "the base is the snapshot below the head");
    assert!(base_bytes > 0, "the base arrived inline");
    assert_eq!(viewer.last_tail, 1, "the step above the base travels as a journal tail");
    assert_eq!(viewer.replica.version, 26);
    eprintln!(
        "deckcraft gate: a third client rebuilt {} KiB at version {base_version} and replayed {} step in {join_ms:.1} ms",
        base_bytes / 1024,
        viewer.last_tail
    );
    let (status, metrics) = http(addr, "GET", "/metrics", None, "");
    assert_eq!(status, 200);
    assert!(metrics.contains("deckcraft_collab_base_transfers_total 1"), "{metrics}");
    assert!(metrics.contains("deckcraft_collab_base_bytes_total "), "{metrics}");

    // Bob drops off; Alice keeps editing. Compaction does not reach these steps, so the
    // reconnect is a journal catch-up, not another base.
    bob.hang_up();
    version += 1;
    alice.submit(move_to("soak-m7", ID, first.clone(), 420.0));
    step([&mut alice, &mut viewer], version);
    version += 1;
    alice.submit(fill_to("soak-f7", ID, first.clone(), "#00ff00"));
    step([&mut alice, &mut viewer], version);

    bob.reconnect(addr, ID, "editor");
    assert_eq!(bob.client.version(), 28, "the journal tail caught the editor up");
    assert!(bob.last_base.is_none(), "a client inside the retained tail needs no base");
    assert_eq!(bob.last_tail, 2, "both missed steps arrived as the tail");

    assert_converged([&alice, &bob, &viewer], 28);
    assert_committed_gate_deck(addr, ID, 28, 420.0, 350.0);
    handle.shutdown();
}

// ---------------------------------------------------------------- the big deck with media

const BIG_SLIDES: usize = 120;
/// Side of a square picture. 160×160 of noise encodes to ~100 KiB, so 120 distinct pictures push
/// the native base past the 8 MiB inline limit (measured: ~12 MiB).
const BIG_SIDE: usize = 160;

/// A square PNG of pseudo-random pixels: noise does not compress, so every slide really adds
/// weight to the base. The generator is a local LCG — a gate must not depend on a random crate to
/// stay reproducible.
fn noise_png(n: usize) -> Vec<u8> {
    let mut pixels = vec![0u8; BIG_SIDE * BIG_SIDE * 4];
    let mut state = 0x2545_f491_4f6c_dd1du64 ^ (n as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    for px in pixels.chunks_mut(4) {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        px[0] = (state >> 24) as u8;
        px[1] = (state >> 16) as u8;
        px[2] = (state >> 8) as u8;
        px[3] = 255;
    }
    deckcraft_render::Image { width: BIG_SIDE as u32, height: BIG_SIDE as u32, pixels }.to_png()
}

/// The initial `.pptx` of the big room: `BIG_SLIDES` slides, each with its own picture and a
/// rectangle (so text, fill and bounds edits have a second object per slide).
fn media_deck() -> (Vec<u8>, f64) {
    let started = Instant::now();
    let pictures: Vec<Vec<u8>> = (0..BIG_SLIDES).map(noise_png).collect();
    let mut session = Session::with_new();
    for (i, picture) in pictures.iter().enumerate() {
        if i > 0 {
            session.apply_remote("slide.new", &json!({"layout": "blank", "at": i})).expect("slide");
        }
        let data = base64::engine::general_purpose::STANDARD.encode(picture);
        session
            .apply_remote("insert.picture", &json!({"name": format!("noise-{i}.png"), "data": data, "rect": [40.0, 60.0, 480.0, 270.0]}))
            .expect("picture");
        session.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [560.0, 80.0, 300.0, 120.0]})).expect("rect");
    }
    let doc = session.doc().expect("doc").doc.as_ref().clone();
    let bytes = deckcraft_pptx::export(&doc).expect("export the big deck");
    (bytes, started.elapsed().as_secs_f64() * 1000.0)
}

/// The wire target of a slide's picture or of its rectangle.
fn media_target(replica: &Replica, slide_index: usize, picture: bool) -> Target {
    let doc = replica.doc();
    let slide = doc.slides.get(slide_index).expect("slide");
    let shape = slide
        .shapes
        .iter()
        .find(|shape| matches!(shape.kind, ShapeKind::Picture { .. }) == picture && shape.xfrm.is_some())
        .expect("the shape the gate edits");
    let keys = replica.adapter.keys();
    Target {
        slide: Some(keys.slide_key(slide.id).expect("slide key").clone()),
        shape: Some(keys.shape_key(slide.id, shape.id).expect("shape key").clone()),
        ..Target::default()
    }
}

/// Every replica holds one document, at the head, with no failed command — for any number of
/// participants.
fn assert_all_converged(peers: &[&Live], version: u64) {
    for live in peers {
        assert_eq!(live.client.version(), version, "{} settled at the session head", live.actor);
        assert_eq!(live.replica.version, version, "{} replica is at the head", live.actor);
        assert_eq!(live.replica.error, None, "{} applied every confirmed command", live.actor);
    }
    for live in peers.iter().skip(1) {
        assert_eq!(live.replica.doc(), peers[0].replica.doc(), "{} holds the same document", live.actor);
    }
}

/// `steps` edits by three editors on a 120-slide deck: move and bounds on the slide's picture,
/// text and fill on its rectangle, each on a slide that comes round again. Returns the head.
fn soak_the_big_deck(peers: &mut [&mut Live; 3], pictures: &[Target], rectangles: &[Target], id: &str, steps: usize) -> u64 {
    let mut version = 0u64;
    for step in 0..steps {
        let slide = step % BIG_SLIDES;
        let op = match step % 4 {
            0 => move_to(&format!("big-{step}"), id, pictures[slide].clone(), 120.0 + (step % 40) as f64),
            1 => bounds_to(&format!("big-{step}"), id, pictures[slide].clone(), 130.0, 90.0, 400.0 + (step % 7) as f64, 240.0),
            2 => text_to(&format!("big-{step}"), id, rectangles[slide].clone(), &format!("slide {slide} step {step}")),
            _ => fill_to(&format!("big-{step}"), id, rectangles[slide].clone(), &format!("#{:02x}40{:02x}", (step % 200) as u8, (step % 90) as u8)),
        };
        version += 1;
        peers[step % 3].submit(op);
        for peer in peers.iter_mut() {
            expect_applied(peer, version);
        }
    }
    version
}

/// A participant that joins on the chained base, drops after three chunks and resumes at the
/// chunk it is missing instead of starting the chain over.
fn a_fifth_participant_resumes_the_chain(addr: SocketAddr, id: &str, initial: &[u8], chunks: usize, base_version: u64) -> Live {
    let mut dave = Live::connect(addr, id, "dave", "editor", initial);
    assert!(dave.pump_one(), "the join asks for the state");
    assert_eq!(dave.last_chunks, Some(chunks), "the same chain");
    for _ in 0..3 {
        assert!(dave.pump_one(), "a chunk");
    }
    let resumed_at = dave.last_chunk_request.expect("the chunk just asked for") + 1;
    dave.hang_up();
    dave.reopen(addr, id, "editor");
    let Some(msg @ ClientMsg::BaseChunk { version: asked_version, index }) = dave.client.next_outgoing() else {
        panic!("the reconnected client asks for the chunk it is missing");
    };
    assert_eq!((asked_version, index), (base_version, resumed_at), "the chain resumes where it stopped");
    dave.send(msg);
    let answer = dave.recv();
    assert!(matches!(answer, ServerMsg::BaseChunk { .. }), "the resumed chunk arrives, not {answer:?}");
    dave.pump_catch_up();
    assert_eq!(dave.last_chunks, Some(chunks));
    assert_eq!(dave.last_tail as u64, dave.client.version() - base_version, "the tail above the base follows the chain");
    dave
}

/// Commit through the real endpoint and check the file reimports with the values every replica
/// holds, slide by slide.
fn commit_the_big_deck(addr: SocketAddr, id: &str, version: u64, reference: &Live) {
    let committing = Instant::now();
    let (status, body) = http(addr, "POST", &format!("/v1/sessions/{id}/commit"), Some("service-token"), "");
    assert_eq!(status, 200, "{body}");
    let answer: Value = serde_json::from_str(&body).expect("json");
    assert_eq!(answer["version"].as_u64(), Some(version));
    let committed = base64::engine::general_purpose::STANDARD.decode(answer["bytes"].as_str().expect("inline bytes")).expect("decode");
    eprintln!(
        "deckcraft gate: commit at version {version} built {} KiB in {:.0} ms",
        committed.len() / 1024,
        committing.elapsed().as_secs_f64() * 1000.0
    );
    let reopened = deckcraft_pptx::import(&committed).expect("the committed bytes reimport");
    assert_eq!(reopened.slides.len(), BIG_SLIDES);
    assert_eq!(picture_count(&reopened), BIG_SLIDES, "every picture survived the file");
    let picture_xfrm = |doc: &Presentation, slide: usize| {
        doc.slides[slide].shapes.iter().find(|shape| matches!(shape.kind, ShapeKind::Picture { .. })).and_then(|shape| shape.xfrm)
    };
    for slide in 0..BIG_SLIDES {
        assert_eq!(picture_xfrm(&reopened, slide), picture_xfrm(reference.replica.doc(), slide), "slide {slide} kept the value every replica holds");
    }
}

/// Three editors make hundreds of edits across a 120-slide deck with media, so the journal is
/// compacted many times and the native base grows past the inline limit. A viewer that joins after
/// all of it is rebuilt from the base **sent in chunks**, a fifth participant drops mid-chain and
/// resumes at the missing chunk, and the commit at the end reimports with the final values.
#[test]
fn a_hundred_and_twenty_slide_deck_with_media_soaks_for_hundreds_of_steps_and_a_chained_base() {
    const ID: &str = "gate-big";
    const STEPS: usize = 322;
    const SNAPSHOT_EVERY: u64 = 40;
    const CHUNK: usize = 256 << 10;
    let (initial, build_ms) = media_deck();
    eprintln!(
        "deckcraft gate: built {BIG_SLIDES} slides with {} media objects into {} KiB of .pptx in {build_ms:.0} ms",
        BIG_SLIDES,
        initial.len() / 1024
    );

    let mut config = deckcraft_collab_server::Config::for_tests(SECRET, "service-token");
    config.snapshot_every = SNAPSHOT_EVERY;
    config.max_base_chunk = CHUNK;
    let handle = deckcraft_collab_server::serve_ephemeral(config).expect("server");
    let addr = handle.addr;
    create(addr, ID, &initial);

    let opened = Instant::now();
    let mut alice = Live::join(addr, ID, "alice", "editor", &initial);
    let mut bob = Live::join(addr, ID, "bob", "editor", &initial);
    let mut carol = Live::join(addr, ID, "carol", "editor", &initial);
    eprintln!("deckcraft gate: three editors opened the room in {:.0} ms", opened.elapsed().as_secs_f64() * 1000.0);
    let pictures: Vec<Target> = (0..BIG_SLIDES).map(|i| media_target(&alice.replica, i, true)).collect();
    let rectangles: Vec<Target> = (0..BIG_SLIDES).map(|i| media_target(&alice.replica, i, false)).collect();

    let soak = Instant::now();
    let mut version = {
        let mut peers: [&mut Live; 3] = [&mut alice, &mut bob, &mut carol];
        soak_the_big_deck(&mut peers, &pictures, &rectangles, ID, STEPS)
    };
    let soak_s = soak.elapsed().as_secs_f64();
    eprintln!(
        "deckcraft gate: {version} edits by three editors on {BIG_SLIDES} slides in {soak_s:.2} s ({:.1} ms per step), \
         snapshots every {SNAPSHOT_EVERY} steps",
        soak_s * 1000.0 / STEPS as f64
    );
    assert_eq!(version, STEPS as u64);
    let base_version = (STEPS as u64) / SNAPSHOT_EVERY * SNAPSHOT_EVERY;
    assert_eq!(base_version, 320, "the last compaction is at 320; two steps sit above it");
    assert_eq!(alice.client.version(), STEPS as u64);

    // A viewer joins at version 0: the journal no longer covers it, the native base is above the
    // inline limit, so it arrives as a chain of chunks and the tail above it follows.
    let joined = Instant::now();
    let mut viewer = Live::join(addr, ID, "victor", "viewer", &initial);
    let join_ms = joined.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(viewer.client.version(), STEPS as u64);
    let (viewer_base_version, base_bytes) = viewer.last_base.expect("the viewer received a base");
    assert_eq!(viewer_base_version, base_version, "the base is the snapshot below the head");
    assert!(base_bytes > 8 << 20, "the native base is above the 8 MiB inline limit: {base_bytes} B");
    let chunks = viewer.last_chunks.expect("the base travels as a chain");
    assert_eq!(chunks, base_bytes.div_ceil(CHUNK), "one chunk per 256 KiB");
    assert_eq!(viewer.last_tail as u64, STEPS as u64 - base_version, "the steps above the base travel as a tail");
    eprintln!(
        "deckcraft gate: a viewer rebuilt {:.1} MiB of base in {chunks} chunks plus {} step(s) of tail in {join_ms:.1} ms",
        base_bytes as f64 / (1024.0 * 1024.0),
        viewer.last_tail
    );
    let (status, metrics) = http(addr, "GET", "/metrics", None, "");
    assert_eq!(status, 200);
    assert!(!metrics.contains("deckcraft_collab_base_chunks_total 0"), "the server counts the chunks it served: {metrics}");
    assert!(metrics.contains("deckcraft_collab_base_bytes_total 0"), "a chained base moves no inline bytes: {metrics}");

    let mut dave = a_fifth_participant_resumes_the_chain(addr, ID, &initial, chunks, base_version);

    // The editors keep working while the two newcomers settle; every participant receives the
    // same two steps and everyone converges.
    version += 1;
    alice.submit(move_to("big-final-move", ID, pictures[0].clone(), 222.0));
    for peer in [&mut alice, &mut bob, &mut carol] {
        expect_applied(peer, version);
    }
    expect_applied(&mut viewer, version);
    expect_applied(&mut dave, version);
    version += 1;
    bob.submit(bounds_to("big-final-bounds", ID, pictures[1].clone(), 240.0, 100.0, 300.0, 200.0));
    for peer in [&mut alice, &mut bob, &mut carol] {
        expect_applied(peer, version);
    }
    expect_applied(&mut viewer, version);
    expect_applied(&mut dave, version);
    let all: [&Live; 5] = [&alice, &bob, &carol, &viewer, &dave];
    assert_all_converged(&all, version);
    assert_eq!(all[0].replica.doc().slides.len(), BIG_SLIDES);
    assert_eq!(picture_count(all[0].replica.doc()), BIG_SLIDES);
    commit_the_big_deck(addr, ID, version, &alice);
    handle.shutdown();
}

// ---------------------------------------------------------------- LibreOffice gate

/// The file the service produces for a 120-slide room with media, through the real commit
/// endpoint (`POST /v1/sessions/{id}/commit`, no cabinet: bytes come back inline).
fn committed_by_the_service() -> Vec<u8> {
    let (doc, _) = large_deck();
    let initial = deckcraft_pptx::export(&doc).expect("export the initial deck");
    let handle = deckcraft_collab_server::serve_ephemeral(deckcraft_collab_server::Config::for_tests(SECRET, "service-token")).expect("server");
    let addr = handle.addr;
    create(addr, "gate", &initial);
    let mut editor = join_socket(addr, "gate", "alice", "editor");
    insert_slide(&mut editor, "gate");
    let (status, body) = http(addr, "POST", "/v1/sessions/gate/commit", Some("service-token"), "");
    assert_eq!(status, 200, "{body}");
    let answer: Value = serde_json::from_str(&body).expect("json");
    assert_eq!(answer["version"].as_u64(), Some(1), "the inserted slide is in the committed version");
    let bytes = base64::engine::general_purpose::STANDARD.decode(answer["bytes"].as_str().expect("inline bytes")).expect("decode");
    handle.shutdown();
    bytes
}

fn create(addr: SocketAddr, id: &str, initial: &[u8]) {
    let body = json!({
        "fileId": id,
        "app": "deckcraft",
        "initialName": format!("{id}.pptx"),
        "initialBytes": base64::engine::general_purpose::STANDARD.encode(initial),
    })
    .to_string();
    let (status, answer) = http(addr, "POST", "/v1/sessions", Some("service-token"), &body);
    assert_eq!(status, 201, "{answer}");
}

fn http(addr: SocketAddr, method: &str, path: &str, token: Option<&str>, body: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).expect("connect");
    let auth = token.map(|token| format!("Authorization: Bearer {token}\r\n")).unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: test\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).expect("write");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read");
    let status = response.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    (status, response.split("\r\n\r\n").nth(1).unwrap_or_default().to_string())
}

fn join_socket(addr: SocketAddr, id: &str, actor: &str, role: &str) -> Client {
    let (mut ws, _) = tungstenite::connect(format!("ws://{addr}/v1/sessions/{id}/ws")).expect("ws");
    let hello = ClientMsg::Hello(deckcraft_collab_proto::Hello {
        v: PROTO_VERSION,
        app: "deckcraft".into(),
        doc: id.into(),
        jwt: token(actor, role, id),
        client: ClientInfo { name: actor.into(), ver: "1".into() },
        last_version: Some(0),
    });
    ws.send(tungstenite::Message::Text(deckcraft_collab_proto::encode_client(&hello).expect("encode").into())).expect("hello");
    match read_socket(&mut ws) {
        ServerMsg::Welcome(welcome) => assert_eq!(welcome.actor_id, actor),
        other => panic!("expected welcome, got {other:?}"),
    }
    ws
}

fn token(actor: &str, role: &str, file: &str) -> String {
    let claims = deckcraft_collab_server::TokenClaims {
        sub: actor.into(),
        role: role.into(),
        file_id: Some(file.into()),
        app: Some("deckcraft".into()),
        session_id: Some(file.into()),
        aud: Some("office-sync".into()),
        exp: Some(now() + 300),
    };
    jsonwebtoken::encode(&Header::new(Algorithm::HS256), &claims, &EncodingKey::from_secret(SECRET)).expect("token")
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn read_socket(ws: &mut Client) -> ServerMsg {
    loop {
        match ws.read().expect("read") {
            tungstenite::Message::Text(text) => return deckcraft_collab_proto::decode_server(text.as_str()).expect("decode"),
            tungstenite::Message::Ping(_) | tungstenite::Message::Pong(_) => {}
            other => panic!("unexpected frame {other:?}"),
        }
    }
}

fn insert_slide(ws: &mut Client, id: &str) {
    let op = ClientMsg::Op(OpEnvelope {
        op_id: "gate-slide".into(),
        doc: id.into(),
        base_version: 0,
        kind: OpKind::Structural,
        target: Default::default(),
        payload: Payload::SlideInsert { before: None, layout: Some("blank".into()) },
        history: None,
        preview: None,
    });
    ws.send(tungstenite::Message::Text(deckcraft_collab_proto::encode_client(&op).expect("encode").into())).expect("send op");
    match read_socket(ws) {
        ServerMsg::Applied(applied) => assert_eq!(applied.version, 1),
        other => panic!("expected applied, got {other:?}"),
    }
}

fn soffice() -> Option<PathBuf> {
    let candidate = PathBuf::from("soffice");
    match Command::new(&candidate).arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status() {
        Ok(status) if status.success() => Some(candidate),
        _ => None,
    }
}

/// A unique scratch directory, removed by [`Scratch::drop`].
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("deckcraft-gate-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run one LibreOffice headless conversion in its own profile; returns the converted file.
fn convert(input: &Path, format: &str, outdir: &Path, profile: &Path) -> PathBuf {
    std::fs::create_dir_all(outdir).expect("outdir");
    std::fs::create_dir_all(profile).expect("profile");
    let mut command = Command::new("soffice");
    command
        .arg("--headless")
        .arg("--norestore")
        .arg("--nolockcheck")
        .arg("--nodefault")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .arg("--convert-to")
        .arg(format)
        .arg("--outdir")
        .arg(outdir)
        .arg(input)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (status, stdout, stderr) = run_with_timeout(command, Duration::from_secs(600));
    let logged = format!("{stdout}\n{stderr}");
    assert!(status, "LibreOffice `--convert-to {format}` failed:\n{logged}");
    let lowered = logged.to_lowercase();
    assert!(!lowered.contains("error:"), "LibreOffice reported an error:\n{logged}");
    assert!(!lowered.contains("could not be loaded"), "LibreOffice could not load the file:\n{logged}");
    let output = outdir.join(input.file_stem().expect("stem")).with_extension(format);
    assert!(output.exists(), "LibreOffice wrote no {}:\n{}", output.display(), logged);
    assert!(std::fs::metadata(&output).expect("metadata").len() > 0, "LibreOffice wrote an empty file");
    output
}

/// `true` when the process exited successfully before the deadline. A hung LibreOffice is killed.
fn run_with_timeout(mut command: Command, timeout: Duration) -> (bool, String, String) {
    let mut child = command.spawn().expect("spawn soffice");
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            let out = child.wait_with_output().expect("output");
            return (status.success(), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return (false, String::new(), format!("timed out after {} s", timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn pdf_pages(path: &Path) -> usize {
    let out = Command::new("pdfinfo").arg(path).output().expect("pdfinfo");
    assert!(out.status.success(), "pdfinfo failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("Pages:"))
        .and_then(|value| value.trim().parse().ok())
        .expect("a page count")
}

#[test]
fn the_committed_presentation_opens_in_libreoffice_without_repair() {
    if soffice().is_none() {
        eprintln!("deckcraft gate: LibreOffice (soffice) is not installed in this environment; the commit gate was skipped");
        return;
    }
    let bytes = committed_by_the_service();
    assert!(bytes.starts_with(b"PK"), "the service answered with a .pptx");
    let scratch = Scratch::new();
    let deck = scratch.path().join("committed.pptx");
    std::fs::write(&deck, &bytes).expect("write commit");

    // Open and render: a file LibreOffice had to repair, or could not parse, fails here.
    let started = Instant::now();
    let pdf = convert(&deck, "pdf", &scratch.path().join("pdf"), &scratch.path().join("profile-pdf"));
    let render_s = started.elapsed().as_secs_f64();
    let pages = pdf_pages(&pdf);
    eprintln!("deckcraft gate: LibreOffice rendered {pages} pages from {} KiB in {render_s:.1} s", bytes.len() / 1024);
    assert_eq!(pages, SLIDES + 1, "every slide, including the one inserted in the session, is a page");

    // Round trip through LibreOffice and back into our importer: what LO understood is a deck of
    // the same shape, with its pictures.
    let started = Instant::now();
    let round = convert(&deck, "pptx", &scratch.path().join("pptx"), &scratch.path().join("profile-pptx"));
    let round_s = started.elapsed().as_secs_f64();
    let reopened = deckcraft_pptx::import(&std::fs::read(&round).expect("read round trip")).expect("the LibreOffice copy reimports");
    eprintln!("deckcraft gate: LibreOffice round trip took {round_s:.1} s");
    assert_eq!(reopened.slides.len(), SLIDES + 1, "LibreOffice kept every slide");
    assert_eq!(picture_count(&reopened), SLIDES, "LibreOffice kept every picture");
}
