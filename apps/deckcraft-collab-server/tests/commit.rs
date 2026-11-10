//! The cabinet callback: a commit in the collaboration server becomes a new `.pptx` file
//! version in the cabinet, the original version stays, and a file changed outside the session is
//! refused with `409` (ADR-0009 §3–4).
//!
//! A fake cabinet on loopback keeps the versions it is given, so the test sees both sides of the
//! contract: the bytes the server posts and the version the cabinet answers with.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use base64::Engine as _;
use deckcraft_collab_proto::{ClientInfo, ClientMsg, Hello, OpEnvelope, OpKind, PROTO_VERSION, Payload, ServerMsg, decode_server, encode_client};
use deckcraft_collab_server::cabinet::CabinetConfig;
use deckcraft_collab_server::{Config, Handle, TokenClaims, serve_ephemeral};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use tungstenite::stream::MaybeTlsStream;

type Client = tungstenite::WebSocket<MaybeTlsStream<TcpStream>>;

const SECRET: &[u8] = b"test-secret-for-deckcraft-collab";

// ---------------------------------------------------------------- fake cabinet

#[derive(Clone, Debug)]
struct Version {
    etag: String,
    name: Option<String>,
    bytes: Vec<u8>,
    sha256: String,
    /// What the server said it expected when it posted this version.
    expected: Option<String>,
    refused: bool,
}

#[derive(Default)]
struct CabinetState {
    files: HashMap<String, Vec<Version>>,
}

struct FakeCabinet {
    addr: SocketAddr,
    state: Arc<Mutex<CabinetState>>,
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl FakeCabinet {
    /// One file per id, each seeded with `bytes` as version 0 and etag `etag-0`.
    fn start(files: &[(&str, &[u8])]) -> Self {
        let mut state = CabinetState::default();
        for (file_id, bytes) in files {
            let version = Version {
                etag: "etag-0".into(),
                name: Some(format!("{file_id}.pptx")),
                bytes: bytes.to_vec(),
                sha256: sha256(bytes),
                expected: None,
                refused: false,
            };
            state.files.insert((*file_id).to_string(), vec![version]);
        }
        let state = Arc::new(Mutex::new(state));
        let stop = Arc::new(AtomicBool::new(false));
        let listener = TcpListener::bind("127.0.0.1:0").expect("cabinet listener");
        listener.set_nonblocking(true).expect("nonblocking");
        let addr = listener.local_addr().expect("address");
        let shared = state.clone();
        let flag = stop.clone();
        let join = thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if let Some(reply) = accept_commit(&mut stream, &shared) {
                            let _ = stream.write_all(&reply);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(2)),
                    Err(_) => thread::sleep(Duration::from_millis(20)),
                }
            }
        });
        FakeCabinet { addr, state, stop, join: Some(join) }
    }

    fn versions(&self, file_id: &str) -> Vec<Version> {
        self.state.lock().expect("state").files.get(file_id).cloned().unwrap_or_default()
    }

    fn attempts(&self, file_id: &str) -> usize {
        self.state.lock().expect("state").files.get(file_id).map(|versions| versions.len()).unwrap_or(0)
    }

    /// The file changed through another client (WebDAV, another editor) while the session ran.
    fn external_change(&self, file_id: &str) {
        let mut state = self.state.lock().expect("state");
        let Some(versions) = state.files.get_mut(file_id) else { return };
        versions.push(Version {
            etag: "etag-ext".into(),
            name: Some(format!("{file_id}.pptx")),
            bytes: b"another client wrote this".to_vec(),
            sha256: sha256(b"another client wrote this"),
            expected: None,
            refused: false,
        });
    }
}

impl Drop for FakeCabinet {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Read one POST and answer it: `200 {version, etag}` when `expectedVersion` matches, `409`
/// otherwise. A refused attempt is recorded, so the test proves the server did post it.
fn accept_commit(stream: &mut TcpStream, state: &Arc<Mutex<CabinetState>>) -> Option<Vec<u8>> {
    let mut request = Vec::new();
    let mut tmp = [0u8; 8192];
    let body_start = loop {
        let n = stream.read(&mut tmp).ok()?;
        if n == 0 {
            return None;
        }
        request.extend_from_slice(&tmp[..n]);
        if let Some(split) = request.windows(4).position(|w| w == b"\r\n\r\n") {
            break split + 4;
        }
    };
    let head = String::from_utf8_lossy(&request[..body_start]).to_ascii_lowercase();
    let length =
        head.lines().find_map(|line| line.strip_prefix("content-length:").map(|value| value.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
    while request.len() < body_start + length {
        let n = stream.read(&mut tmp).ok()?;
        if n == 0 {
            break;
        }
        request.extend_from_slice(&tmp[..n]);
    }
    let body: serde_json::Value = serde_json::from_slice(&request[body_start..]).ok()?;
    let file_id = body.get("fileId")?.as_str()?.to_string();
    let name = body.get("name").and_then(|name| name.as_str()).map(str::to_string);
    let expected = body.get("expectedVersion").and_then(|value| value.as_str()).map(str::to_string);
    let bytes = body.get("bytes")?.as_str().and_then(|value| base64::engine::general_purpose::STANDARD.decode(value).ok())?;
    let sha256 = body.get("sha256")?.as_str()?.to_string();

    let mut state = state.lock().ok()?;
    let Some(versions) = state.files.get_mut(&file_id) else {
        return Some(reply(404, "Not Found", "{\"error\":\"no such file\"}"));
    };
    let current = versions.last()?.etag.clone();
    let wanted = expected.clone().unwrap_or_else(|| current.clone());
    if wanted != current {
        versions.push(Version { etag: current.clone(), name, bytes: Vec::new(), sha256, expected, refused: true });
        return Some(reply(409, "Conflict", "{\"error\":\"another version\"}"));
    }
    let number = versions.len() as u64;
    let etag = format!("etag-{number}");
    versions.push(Version { etag: etag.clone(), name, bytes, sha256, expected, refused: false });
    Some(reply(200, "OK", &format!("{{\"version\":{number},\"etag\":\"{etag}\"}}")))
}

fn reply(status: u16, reason: &str, body: &str) -> Vec<u8> {
    format!("HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
        .into_bytes()
}

// ---------------------------------------------------------------- deckcraft server

fn start(cabinet: &FakeCabinet) -> Handle {
    let mut config = Config::for_tests(SECRET, "service-token");
    config.cabinet = Some(CabinetConfig {
        url: format!("http://{}/api/v1/internal/office/sessions/{{sessionId}}/commit", cabinet.addr),
        token: "cabinet-token".into(),
        timeout: Duration::from_secs(5),
    });
    serve_ephemeral(config).expect("server")
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
    let body = response.split("\r\n\r\n").nth(1).unwrap_or_default().to_string();
    (status, body)
}

fn native_deck() -> Vec<u8> {
    let session = session_with_shape();
    deckcraft_format::save(session.doc().expect("doc").doc.as_ref()).expect("save")
}

fn original_shape_count() -> usize {
    session_with_shape().doc().expect("doc").doc.slides[0].shapes.len()
}

fn session_with_shape() -> deckcraft_engine::Session {
    let mut session = deckcraft_engine::Session::with_new();
    session.apply_remote("shape.insert", &serde_json::json!({"preset": "rect", "rect": [100, 100, 200, 100]})).expect("shape");
    session
}

fn create(addr: SocketAddr, file_id: &str, bytes: &[u8], expected: &str, name: &str) -> String {
    let body = serde_json::json!({
        "fileId": file_id,
        "app": "deckcraft",
        "initialName": name,
        "initialBytes": base64::engine::general_purpose::STANDARD.encode(bytes),
        "expectedVersion": expected,
    })
    .to_string();
    let (status, created) = http(addr, "POST", "/v1/sessions", Some("service-token"), &body);
    assert_eq!(status, 201, "{created}");
    serde_json::from_str::<serde_json::Value>(&created).expect("json")["sessionId"].as_str().expect("id").to_string()
}

fn token(actor: &str, role: &str, file: &str) -> String {
    let claims = TokenClaims {
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

fn join(addr: SocketAddr, id: &str, actor: &str, role: &str) -> Client {
    let (mut ws, _) = tungstenite::connect(format!("ws://{addr}/v1/sessions/{id}/ws")).expect("ws");
    let hello = ClientMsg::Hello(Hello {
        v: PROTO_VERSION,
        app: "deckcraft".into(),
        doc: id.into(),
        jwt: token(actor, role, id),
        client: ClientInfo { name: actor.into(), ver: "1".into() },
        last_version: Some(0),
    });
    ws.send(tungstenite::Message::Text(encode_client(&hello).expect("encode").into())).expect("hello");
    match read(&mut ws) {
        ServerMsg::Welcome(welcome) => assert_eq!(welcome.actor_id, actor),
        other => panic!("expected welcome, got {other:?}"),
    }
    ws
}

fn read(ws: &mut Client) -> ServerMsg {
    loop {
        match ws.read().expect("read") {
            tungstenite::Message::Text(text) => return decode_server(text.as_str()).expect("decode"),
            tungstenite::Message::Ping(_) | tungstenite::Message::Pong(_) => {}
            other => panic!("unexpected frame {other:?}"),
        }
    }
}

fn insert_slide(ws: &mut Client, id: &str, op_id: &str) {
    let op = ClientMsg::Op(OpEnvelope {
        op_id: op_id.into(),
        doc: id.into(),
        base_version: 0,
        kind: OpKind::Structural,
        target: Default::default(),
        payload: Payload::SlideInsert { before: None, layout: Some("blank".into()) },
        history: None,
    });
    ws.send(tungstenite::Message::Text(encode_client(&op).expect("encode").into())).expect("send op");
    match read(ws) {
        ServerMsg::Applied(applied) => assert_eq!(applied.version, 1),
        other => panic!("expected applied, got {other:?}"),
    }
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------- tests

#[test]
fn without_a_callback_the_commit_returns_the_bytes() {
    let handle = serve_ephemeral(Config::for_tests(SECRET, "service-token")).expect("server");
    let addr = handle.addr;
    create(addr, "file-n", &native_deck(), "etag-0", "talk.pptx");
    let (status, result) = http(addr, "POST", "/v1/sessions/file-n/commit", Some("service-token"), "");
    assert_eq!(status, 200, "{result}");
    let result: serde_json::Value = serde_json::from_str(&result).expect("json");
    let bytes = base64::engine::general_purpose::STANDARD.decode(result["bytes"].as_str().expect("bytes")).expect("decode");
    deckcraft_pptx::import(&bytes).expect("the answer carries a .pptx");
    assert_eq!(result["fileVersion"], "etag-0", "no cabinet version was created");
    let (status, _) = http(addr, "POST", "/v1/sessions/file-n/commit", None, "");
    assert_eq!(status, 401, "the commit endpoint needs the service token");
    handle.shutdown();
}

#[test]
fn a_commit_creates_a_new_version_on_both_copies_and_keeps_the_originals() {
    let original = native_deck();
    let cabinet = FakeCabinet::start(&[("file-1", &original), ("file-2", &original)]);
    let handle = start(&cabinet);
    let addr = handle.addr;

    // First copy: one edit, then a commit through the control plane.
    let id = create(addr, "file-1", &original, "etag-0", "talk.pptx");
    assert_eq!(id, "file-1");
    let mut editor = join(addr, &id, "alice", "editor");
    insert_slide(&mut editor, &id, "slide-1");
    let (status, result) = http(addr, "POST", "/v1/sessions/file-1/commit", Some("service-token"), r#"{"expectedVersion":"etag-0"}"#);
    assert_eq!(status, 200, "{result}");
    let result: serde_json::Value = serde_json::from_str(&result).expect("json");
    assert_eq!(result["fileVersion"], "etag-1", "{result}");
    assert_eq!(result["version"], 1, "the document version the file represents");
    assert_eq!(result["name"], "talk.pptx", "the name the cabinet knows the file by");
    assert!(result["warnings"].is_array(), "{result}");

    let first = cabinet.versions("file-1");
    assert_eq!(first.len(), 2, "the original version and the committed one");
    assert_eq!(first[0].bytes, original, "the cabinet's original version is untouched");
    assert_eq!(first[1].sha256, sha256(&first[1].bytes), "the posted digest matches the posted bytes");
    assert_eq!(first[1].expected.as_deref(), Some("etag-0"), "the server said which version it was built against");
    assert_eq!(first[1].sha256, result["sha256"].as_str().expect("sha"), "the answer names the stored bytes");
    assert_eq!(first[1].name.as_deref(), Some("talk.pptx"), "the cabinet stored the name the room was opened with");
    // Round-trip: what the cabinet stored is a presentation with the edit.
    let reopened = deckcraft_pptx::import(&first[1].bytes).expect("the committed bytes reimport");
    assert_eq!(reopened.slides.len(), 2, "the inserted slide survived the file");
    assert_eq!(reopened.slides[0].shapes.len(), original_shape_count(), "the original content is still there");

    // Second copy of the same original file: its own session and its own version, no edit.
    create(addr, "file-2", &original, "etag-0", "copy.pptx");
    let (status, result) = http(addr, "POST", "/v1/sessions/file-2/commit", Some("service-token"), r#"{"expectedVersion":"etag-0"}"#);
    assert_eq!(status, 200, "{result}");
    let second = cabinet.versions("file-2");
    assert_eq!(second.len(), 2, "each copy has its own versions");
    assert_eq!(second[0].bytes, original, "the second original is untouched too");
    let reopened = deckcraft_pptx::import(&second[1].bytes).expect("the second commit reimports");
    assert_eq!(reopened.slides.len(), 1, "the second copy kept its own content");
    assert_eq!(reopened.slides[0].shapes.len(), original_shape_count());

    // A commit with nothing to publish still answers with the accepted version.
    let (status, _) = http(addr, "POST", "/v1/sessions/file-1/commit", Some("service-token"), r#"{"expectedVersion":"etag-1"}"#);
    assert_eq!(status, 200);
    assert_eq!(cabinet.versions("file-1").last().expect("version").expected.as_deref(), Some("etag-1"));

    let (_, metrics) = http(addr, "GET", "/metrics", None, "");
    assert!(metrics.contains("deckcraft_collab_cabinet_commits_total 3"), "{metrics}");
    assert!(metrics.contains("deckcraft_collab_cabinet_conflicts_total 0"), "{metrics}");
    handle.shutdown();
}

#[test]
fn an_external_change_is_a_409_and_nothing_is_overwritten() {
    let original = native_deck();
    let cabinet = FakeCabinet::start(&[("file-x", &original)]);
    let handle = start(&cabinet);
    let addr = handle.addr;
    create(addr, "file-x", &original, "etag-0", "talk.pptx");

    // A commit whose expected version the session never saw is refused before the cabinet is
    // even called.
    let (status, _) = http(addr, "POST", "/v1/sessions/file-x/commit", Some("service-token"), r#"{"expectedVersion":"etag-old"}"#);
    assert_eq!(status, 409);
    assert_eq!(cabinet.attempts("file-x"), 1, "the local check made no callback");

    // The cabinet's file changed under the session: the callback is refused with 409.
    cabinet.external_change("file-x");
    let before = cabinet.versions("file-x").len();
    let (status, body) = http(addr, "POST", "/v1/sessions/file-x/commit", Some("service-token"), r#"{"expectedVersion":"etag-0"}"#);
    assert_eq!(status, 409, "{body}");
    let attempts = cabinet.attempts("file-x");
    assert_eq!(attempts, before + 1, "the server did ask the cabinet");
    let posted = cabinet.versions("file-x");
    assert!(posted.last().expect("attempt").refused, "the attempt was recorded as refused");
    assert!(posted.last().expect("attempt").bytes.is_empty(), "nothing was written");
    assert_eq!(posted[0].bytes, original, "the original version is still the one the cabinet held");

    let (_, metrics) = http(addr, "GET", "/metrics", None, "");
    assert!(metrics.contains("deckcraft_collab_cabinet_conflicts_total 1"), "{metrics}");
    assert!(metrics.contains("deckcraft_collab_cabinet_commits_total 0"), "no version was published: {metrics}");
    handle.shutdown();
}

#[test]
fn a_client_commit_is_answered_after_the_cabinet_took_the_file() {
    let original = native_deck();
    let cabinet = FakeCabinet::start(&[("file-w", &original)]);
    let handle = start(&cabinet);
    let addr = handle.addr;
    create(addr, "file-w", &original, "etag-0", "talk.pptx");

    let mut editor = join(addr, "file-w", "alice", "editor");
    insert_slide(&mut editor, "file-w", "slide-1");
    editor
        .send(tungstenite::Message::Text(encode_client(&ClientMsg::Commit { expected_file_version: Some("etag-0".into()) }).expect("encode").into()))
        .expect("commit");
    match read(&mut editor) {
        ServerMsg::CommitResult(result) => {
            assert_eq!(result.file_version.as_deref(), Some("etag-1"), "the answer carries the version the cabinet stored");
            assert_eq!(result.version, 1);
            let bytes = base64::engine::general_purpose::STANDARD.decode(result.bytes_base64.expect("bytes")).expect("decode");
            assert!(bytes.starts_with(b"PK"), "a .pptx is a zip");
            assert_eq!(result.sha256, sha256(&bytes));
        }
        other => panic!("expected commit-result, got {other:?}"),
    }
    let stored = cabinet.versions("file-w");
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[1].sha256, sha256(&stored[1].bytes));

    // The next commit compares against the version the cabinet just stored.
    let mut stale = join(addr, "file-w", "bob", "editor");
    stale
        .send(tungstenite::Message::Text(encode_client(&ClientMsg::Commit { expected_file_version: Some("etag-0".into()) }).expect("encode").into()))
        .expect("commit");
    match read(&mut stale) {
        ServerMsg::Rejected(rejected) => assert_eq!(rejected.code, deckcraft_collab_proto::RejectCode::Conflict),
        other => panic!("expected conflict, got {other:?}"),
    }

    // A viewer cannot commit at all.
    let mut viewer = join(addr, "file-w", "victor", "viewer");
    viewer
        .send(tungstenite::Message::Text(encode_client(&ClientMsg::Commit { expected_file_version: None }).expect("encode").into()))
        .expect("commit");
    match read(&mut viewer) {
        ServerMsg::Rejected(rejected) => assert_eq!(rejected.code, deckcraft_collab_proto::RejectCode::Forbidden),
        other => panic!("expected forbidden, got {other:?}"),
    }
    handle.shutdown();
}

/// The desktop Save path end to end: a client asks for a commit with no version of its own (it
/// never saw the cabinet), the server compares against the version `meta.json` restored, and
/// after a restart that version still refuses a file changed outside the session (ADR 0009 §4).
#[test]
fn the_accepted_version_survives_a_restart_and_refuses_an_external_change() {
    let original = native_deck();
    let cabinet = FakeCabinet::start(&[("file-s", &original)]);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("deckcraft-commit-restart-{}-{nanos}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut config = Config::for_tests(SECRET, "service-token");
    config.store_dir = Some(dir.clone());
    config.cabinet = Some(CabinetConfig {
        url: format!("http://{}/api/v1/internal/office/sessions/{{sessionId}}/commit", cabinet.addr),
        token: "cabinet-token".into(),
        timeout: Duration::from_secs(5),
    });
    let handle = serve_ephemeral(config.clone()).expect("server");
    let addr = handle.addr;
    create(addr, "file-s", &original, "etag-0", "talk.pptx");

    let mut editor = join(addr, "file-s", "alice", "editor");
    insert_slide(&mut editor, "file-s", "slide-1");
    editor
        .send(tungstenite::Message::Text(encode_client(&ClientMsg::Commit { expected_file_version: None }).expect("encode").into()))
        .expect("commit");
    match read(&mut editor) {
        ServerMsg::CommitResult(result) => {
            assert_eq!(result.file_version.as_deref(), Some("etag-1"), "the cabinet's answer reaches the client");
            assert_eq!(result.version, 1);
        }
        other => panic!("expected commit-result, got {other:?}"),
    }
    drop(editor);
    handle.shutdown();

    // `meta.json` names the version the cabinet accepted.
    let room = dir.join(sha256(b"file-s"));
    let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(room.join("meta.json")).expect("meta.json")).expect("json");
    assert_eq!(meta["fileEtag"], "etag-1", "{meta}");

    // A restart restores the room (a create without an expected version must not forget the
    // version the cabinet holds); the file then changes outside the session.
    let handle = serve_ephemeral(config).expect("restart");
    let (status, info) = http(handle.addr, "POST", "/v1/sessions", Some("service-token"), "{\"fileId\":\"file-s\",\"app\":\"deckcraft\"}");
    assert_eq!(status, 201, "{info}");
    cabinet.external_change("file-s");
    let (status, body) = http(handle.addr, "POST", "/v1/sessions/file-s/commit", Some("service-token"), "");
    assert_eq!(status, 409, "a commit with no expected version still compares against the restored one: {body}");
    let posted = cabinet.versions("file-s");
    assert!(posted.last().expect("attempt").refused, "the attempt was refused");
    assert!(posted.last().expect("attempt").bytes.is_empty(), "nothing was written over the external change");
    assert_eq!(posted[posted.len() - 2].etag, "etag-ext", "the external version is still the one the cabinet holds");
    handle.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}
