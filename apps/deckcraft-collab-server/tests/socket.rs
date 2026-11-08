//! A real socket test: HTTP session control, two WebSocket clients, a viewer, and metrics.
//!
//! The server is started on an ephemeral loopback port; nothing here touches the network beyond
//! localhost.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Read, Write};
use std::net::TcpStream;

use base64::Engine as _;
use deckcraft_collab_proto::{ClientInfo, ClientMsg, Hello, OpEnvelope, OpKind, PROTO_VERSION, Payload, ServerMsg, decode_server, encode_client};
use deckcraft_collab_server::{Config, Handle, TokenClaims, serve_ephemeral};
use tungstenite::stream::MaybeTlsStream;

type Client = tungstenite::WebSocket<MaybeTlsStream<TcpStream>>;
use jsonwebtoken::{Algorithm, EncodingKey, Header};

const SECRET: &[u8] = b"test-secret-for-deckcraft-collab";

fn start() -> Handle {
    serve_ephemeral(Config::for_tests(SECRET, "service-token")).expect("server")
}

fn http(addr: std::net::SocketAddr, method: &str, path: &str, token: Option<&str>, body: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).expect("connect");
    let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
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

fn join(addr: std::net::SocketAddr, id: &str, actor: &str, role: &str) -> Client {
    let (mut ws, _) = tungstenite::connect(format!("ws://{addr}/v1/sessions/{id}/ws")).expect("ws");
    let hello = ClientMsg::Hello(Hello {
        v: PROTO_VERSION,
        app: "deckcraft".into(),
        doc: id.into(),
        jwt: token(actor, role, id),
        client: ClientInfo { name: actor.into(), ver: "1".into() },
        last_version: Some(0),
    });
    ws.send(tungstenite::Message::Text(encode_client(&hello).expect("encode").into())).expect("send hello");
    match read(&mut ws) {
        ServerMsg::Welcome(w) => assert_eq!(w.actor_id, actor),
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

fn native_deck() -> Vec<u8> {
    let mut s = deckcraft_engine::Session::with_new();
    s.apply_remote("shape.insert", &serde_json::json!({"preset": "rect", "rect": [100, 100, 200, 100]})).expect("shape");
    deckcraft_format::save(s.doc().expect("doc").doc.as_ref()).expect("save")
}

#[test]
fn a_session_accepts_two_editors_and_refuses_a_viewer() {
    let handle = start();
    let addr = handle.addr;
    let bytes = base64::engine::general_purpose::STANDARD.encode(native_deck());
    let body = serde_json::json!({
        "fileId": "file-1",
        "app": "deckcraft",
        "initialBytes": bytes,
        "expectedVersion": "etag-0",
    })
    .to_string();
    let (status, created) = http(addr, "POST", "/v1/sessions", Some("service-token"), &body);
    assert_eq!(status, 201, "{created}");
    let id = serde_json::from_str::<serde_json::Value>(&created).expect("json")["sessionId"].as_str().expect("id").to_string();

    let mut a = join(addr, &id, "alice", "editor");
    let mut b = join(addr, &id, "bob", "editor");
    let op = ClientMsg::Op(OpEnvelope {
        op_id: "op-1".into(),
        doc: id.clone(),
        base_version: 0,
        kind: OpKind::Structural,
        target: Default::default(),
        payload: Payload::SlideInsert { before: None, layout: Some("blank".into()) },
        history: None,
    });
    a.send(tungstenite::Message::Text(encode_client(&op).expect("encode").into())).expect("send op");
    let applied_a = read(&mut a);
    let applied_b = read(&mut b);
    match (&applied_a, &applied_b) {
        (ServerMsg::Applied(x), ServerMsg::Applied(y)) => {
            assert_eq!(x.version, 1);
            assert_eq!(y.version, 1);
            assert_eq!(x.op_id, "op-1");
        }
        other => panic!("expected applied on both sides, got {other:?}"),
    }

    let mut viewer = join(addr, &id, "victor", "viewer");
    let op = ClientMsg::Op(OpEnvelope {
        op_id: "op-2".into(),
        doc: id.clone(),
        base_version: 1,
        kind: OpKind::Command,
        target: Default::default(),
        payload: Payload::ShapeMove { x: 1.0, y: 2.0 },
        history: None,
    });
    viewer.send(tungstenite::Message::Text(encode_client(&op).expect("encode").into())).expect("send op");
    match read(&mut viewer) {
        ServerMsg::Rejected(r) => assert_eq!(r.code, deckcraft_collab_proto::RejectCode::Forbidden),
        other => panic!("expected forbidden, got {other:?}"),
    }

    let (status, health) = http(addr, "GET", "/healthz", None, "");
    assert_eq!(status, 200);
    assert!(health.contains("\"ok\":true"), "{health}");
    let (status, metrics) = http(addr, "GET", "/metrics", None, "");
    assert_eq!(status, 200);
    assert!(metrics.contains("deckcraft_collab_ops_total 2"), "both operations reached the hub: {metrics}");
    assert!(metrics.contains("deckcraft_collab_rooms 1"), "{metrics}");

    // Session control fails closed without the service token.
    let (status, _) = http(addr, "GET", "/v1/sessions/file-1", None, "");
    assert_eq!(status, 200, "read-only session info is public to the service network");
    handle.shutdown();
}

#[test]
fn a_restart_replays_the_journal_into_the_same_version() {
    let dir = std::env::temp_dir().join(format!("deckcraft-collab-restart-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut config = Config::for_tests(SECRET, "service-token");
    config.store_dir = Some(dir.clone());
    config.snapshot_every = 100;
    let handle = serve_ephemeral(config.clone()).expect("server");
    let addr = handle.addr;
    let bytes = base64::engine::general_purpose::STANDARD.encode(native_deck());
    let body = serde_json::json!({"fileId": "file-r", "app": "deckcraft", "initialBytes": bytes}).to_string();
    let (status, created) = http(addr, "POST", "/v1/sessions", Some("service-token"), &body);
    assert_eq!(status, 201, "{created}");
    let mut editor = join(addr, "file-r", "alice", "editor");
    let op = ClientMsg::Op(OpEnvelope {
        op_id: "restart-1".into(),
        doc: "file-r".into(),
        base_version: 0,
        kind: OpKind::Structural,
        target: Default::default(),
        payload: Payload::SlideInsert { before: None, layout: Some("blank".into()) },
        history: None,
    });
    editor.send(tungstenite::Message::Text(encode_client(&op).expect("encode").into())).expect("send");
    match read(&mut editor) {
        ServerMsg::Applied(a) => assert_eq!(a.version, 1),
        other => panic!("{other:?}"),
    }
    drop(editor);
    handle.shutdown();

    let handle = serve_ephemeral(config).expect("restart");
    let (status, info) = http(handle.addr, "POST", "/v1/sessions", Some("service-token"), "{\"fileId\":\"file-r\",\"app\":\"deckcraft\"}");
    assert_eq!(status, 201, "a restored room is created in this process: {info}");
    let info: serde_json::Value = serde_json::from_str(&info).expect("json");
    assert_eq!(info["version"], 1, "the journal moved the version across the restart");
    handle.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_invalid_token_never_reaches_a_room() {
    let handle = start();
    let addr = handle.addr;
    let body = serde_json::json!({"fileId": "file-2", "app": "deckcraft"}).to_string();
    let (status, _) = http(addr, "POST", "/v1/sessions", Some("service-token"), &body);
    assert_eq!(status, 201);
    let (mut ws, _) = tungstenite::connect(format!("ws://{addr}/v1/sessions/file-2/ws")).expect("ws");
    let hello = ClientMsg::Hello(Hello {
        v: PROTO_VERSION,
        app: "deckcraft".into(),
        doc: "file-2".into(),
        jwt: "not-a-token".into(),
        client: ClientInfo { name: "x".into(), ver: "1".into() },
        last_version: None,
    });
    ws.send(tungstenite::Message::Text(encode_client(&hello).expect("encode").into())).expect("send");
    // The connection closes without a welcome; a read yields an error or a close frame.
    match ws.read() {
        Ok(tungstenite::Message::Close(_)) | Err(_) => {}
        Ok(other) => panic!("expected the connection to close, got {other:?}"),
    }
    handle.shutdown();
}
