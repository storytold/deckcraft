//! The two gates TASK.md §8 asks for beyond the command slices: a 100+ slide deck with media
//! stays fast through two clients and a commit, and the `.pptx` a commit produces opens in
//! LibreOffice without repair and survives a LibreOffice round trip.
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
use deckcraft_collab::{ClientSession, DocSession, SessionConfig};
use deckcraft_collab_proto::{
    Capabilities, ClientInfo, ClientMsg, Limits, OpEnvelope, OpKind, PROTO_VERSION, Payload, Role, ServerMsg, ShapeKey, SlideKey, SnapshotMode,
    SnapshotRef, Target, Welcome,
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

// ---------------------------------------------------------------- LibreOffice gate

/// The file the service produces for a 120-slide room with media, through the real commit
/// endpoint (`POST /v1/sessions/{id}/commit`, no cabinet: bytes come back inline).
fn committed_by_the_service() -> Vec<u8> {
    let (doc, _) = large_deck();
    let initial = deckcraft_pptx::export(&doc).expect("export the initial deck");
    let handle = deckcraft_collab_server::serve_ephemeral(deckcraft_collab_server::Config::for_tests(SECRET, "service-token")).expect("server");
    let addr = handle.addr;
    create(addr, &initial);
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

fn create(addr: SocketAddr, initial: &[u8]) {
    let body = json!({
        "fileId": "gate",
        "app": "deckcraft",
        "initialName": "gate.pptx",
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
