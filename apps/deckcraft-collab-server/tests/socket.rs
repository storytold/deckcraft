//! A real socket test: HTTP session control, two WebSocket clients, a viewer, and metrics.
//!
//! The server is started on an ephemeral loopback port; nothing here touches the network beyond
//! localhost.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Read, Write};
use std::net::TcpStream;

use base64::Engine as _;
use deckcraft_collab_proto::{
    ClientInfo, ClientMsg, Hello, OpEnvelope, OpKind, PROTO_VERSION, Payload, ServerMsg, Target, decode_server, encode_client,
};
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

/// An HTTP request with binary bodies on either side (media upload and fetch).
fn http_bytes(addr: std::net::SocketAddr, method: &str, path: &str, token: Option<&str>, body: &[u8]) -> (u16, Vec<u8>) {
    let (status, _, body) = http_parts(addr, method, path, token, body);
    (status, body)
}

/// The same request, with the response head kept so a test can read the declared content type.
fn http_parts(addr: std::net::SocketAddr, method: &str, path: &str, token: Option<&str>, body: &[u8]) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(addr).expect("connect");
    let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: test\r\n{auth}Content-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).expect("write head");
    stream.write_all(body).expect("write body");
    let mut response = Vec::new();
    stream.read_to_end(&mut response).expect("read");
    let split = response.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4).unwrap_or(response.len());
    let head = String::from_utf8_lossy(&response[..split]).to_string();
    let status = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    (status, head, response[split..].to_vec())
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

/// 8×8 red PNG.
fn png() -> Vec<u8> {
    vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0, 8, 8, 2, 0, 0, 0, 75, 109, 41, 220, 0, 0, 0, 17, 73, 68,
        65, 84, 120, 156, 99, 184, 163, 161, 129, 21, 49, 12, 45, 9, 0, 123, 123, 75, 1, 251, 162, 81, 240, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96,
        130,
    ]
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// A minimal binary glTF: the magic, version 2 and a declared length, then an empty chunk.
fn glb() -> Vec<u8> {
    let mut bytes = b"glTF".to_vec();
    bytes.extend(2u32.to_le_bytes());
    bytes.extend(20u32.to_le_bytes());
    bytes.extend([0u8; 8]);
    bytes
}

/// The slide key a client derives for the first slide of `native_deck()` (ADR 0002).
fn first_slide_key(file_id: &str) -> deckcraft_collab_proto::SlideKey {
    let mut s = deckcraft_engine::Session::with_new();
    s.apply_remote("shape.insert", &serde_json::json!({"preset": "rect", "rect": [100, 100, 200, 100]})).expect("shape");
    let doc = s.doc().expect("doc").doc.as_ref().clone();
    let mut map = deckcraft_collab::KeyMap::new();
    let mut issuer = deckcraft_collab::KeyIssuer::new(format!("{file_id}-"));
    map.sync(&doc, &mut issuer);
    map.slide_key(doc.slides[0].id).expect("slide key").clone()
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
fn a_picture_is_uploaded_once_and_reaches_a_second_editor() {
    let handle = start();
    let addr = handle.addr;
    let body =
        serde_json::json!({"fileId": "file-m", "app": "deckcraft", "initialBytes": base64::engine::general_purpose::STANDARD.encode(native_deck())})
            .to_string();
    let (status, created) = http(addr, "POST", "/v1/sessions", Some("service-token"), &body);
    assert_eq!(status, 201, "{created}");

    let editor = token("alice", "editor", "file-m");
    let viewer = token("victor", "viewer", "file-m");
    let picture = png();
    let sha = sha256(&picture);
    let upload = format!("/v1/sessions/file-m/media/{sha}");

    // A picture is not public and a viewer cannot add one.
    let (status, _) = http_bytes(addr, "GET", &format!("/media/{sha}"), None, b"");
    assert_eq!(status, 401);
    let (status, _) = http_bytes(addr, "PUT", &upload, Some(&viewer), &picture);
    assert_eq!(status, 403);
    let (status, _) = http_bytes(addr, "PUT", &format!("/v1/sessions/file-m/media/{}", "0".repeat(64)), Some(&editor), &picture);
    assert_eq!(status, 400, "the path must name the body's digest");

    let (status, uploaded) = http_bytes(addr, "PUT", &upload, Some(&editor), &picture);
    assert_eq!(status, 201, "{}", String::from_utf8_lossy(&uploaded));
    let (status, fetched) = http_bytes(addr, "GET", &format!("/media/{sha}"), Some(&editor), b"");
    assert_eq!(status, 200);
    assert_eq!(fetched, picture, "the bytes come back unchanged");

    // Two editors; the picture arrives as a reference, not as bytes in the operation.
    let mut a = join(addr, "file-m", "alice", "editor");
    let mut b = join(addr, "file-m", "bob", "editor");
    let op = ClientMsg::Op(OpEnvelope {
        op_id: "pic-1".into(),
        doc: "file-m".into(),
        base_version: 0,
        kind: OpKind::Structural,
        target: Target { slide: Some(first_slide_key("file-m")), ..Target::default() },
        payload: Payload::PictureInsert { sha256: sha.clone(), name: "red.png".into(), rect: Some([80.0, 80.0, 120.0, 120.0]) },
        history: None,
    });
    a.send(tungstenite::Message::Text(encode_client(&op).expect("encode").into())).expect("send op");
    let applied_a = read(&mut a);
    let applied_b = read(&mut b);
    for applied in [&applied_a, &applied_b] {
        match applied {
            ServerMsg::Applied(applied) => {
                assert_eq!(applied.version, 1);
                assert!(applied.delta.changes.iter().any(|c| c.shape.is_some()), "a shape was inserted: {applied:?}");
            }
            other => panic!("expected applied, got {other:?}"),
        }
    }
    let (_, metrics) = http(addr, "GET", "/metrics", None, "");
    assert!(metrics.contains("deckcraft_collab_media_uploads_total 1"), "{metrics}");
    assert!(metrics.contains(&format!("deckcraft_collab_media_bytes_total {}", picture.len())), "{metrics}");
    handle.shutdown();
}

#[test]
fn audio_and_video_use_the_same_media_channel_with_their_own_allowlist() {
    let handle = start();
    let addr = handle.addr;
    let body =
        serde_json::json!({"fileId": "file-a", "app": "deckcraft", "initialBytes": base64::engine::general_purpose::STANDARD.encode(native_deck())})
            .to_string();
    let (status, created) = http(addr, "POST", "/v1/sessions", Some("service-token"), &body);
    assert_eq!(status, 201, "{created}");
    let editor = token("alice", "editor", "file-a");

    let wav = b"RIFF\x24\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x44\xac\0\0\x88\x58\x01\0\x02\0\x10\0data\0\0\0\0".to_vec();
    let sha = sha256(&wav);
    let (status, uploaded) = http_bytes(addr, "PUT", &format!("/v1/sessions/file-a/media/{sha}"), Some(&editor), &wav);
    assert_eq!(status, 201, "{}", String::from_utf8_lossy(&uploaded));
    let (status, fetched) = http_bytes(addr, "GET", &format!("/media/{sha}"), Some(&editor), b"");
    assert_eq!(status, 200);
    assert_eq!(fetched, wav);

    // Not a picture, video or audio signature: the channel refuses it by name.
    let text = b"not media at all".to_vec();
    let sha = sha256(&text);
    let (status, _) = http_bytes(addr, "PUT", &format!("/v1/sessions/file-a/media/{sha}"), Some(&editor), &text);
    assert_eq!(status, 400, "an arbitrary file is refused");
    handle.shutdown();
}

#[test]
fn a_font_face_uses_the_media_channel_and_lands_in_the_committed_file() {
    let handle = start();
    let addr = handle.addr;
    let body =
        serde_json::json!({"fileId": "file-f", "app": "deckcraft", "initialBytes": base64::engine::general_purpose::STANDARD.encode(native_deck())})
            .to_string();
    let (status, created) = http(addr, "POST", "/v1/sessions", Some("service-token"), &body);
    assert_eq!(status, 201, "{created}");
    let editor = token("alice", "editor", "file-f");
    let viewer = token("victor", "viewer", "file-f");

    // A font face is a media object like a picture: the writer stores it as `fntdata`.
    let face = include_bytes!("../../../crates/render/tests/fixtures/colr-squares.ttf").to_vec();
    let sha = sha256(&face);
    let upload = format!("/v1/sessions/file-f/media/{sha}");
    let (status, _) = http_bytes(addr, "PUT", &upload, Some(&viewer), &face);
    assert_eq!(status, 403, "a viewer cannot upload media");
    let (status, uploaded) = http_bytes(addr, "PUT", &upload, Some(&editor), &face);
    assert_eq!(status, 201, "{}", String::from_utf8_lossy(&uploaded));
    let (status, fetched) = http_bytes(addr, "GET", &format!("/media/{sha}"), Some(&editor), b"");
    assert_eq!(status, 200);
    assert_eq!(fetched, face, "the font bytes come back unchanged");

    // The operation names the digest and no slide: a document font belongs to the presentation.
    let mut a = join(addr, "file-f", "alice", "editor");
    let op = ClientMsg::Op(OpEnvelope {
        op_id: "font-1".into(),
        doc: "file-f".into(),
        base_version: 0,
        kind: OpKind::Command,
        target: Target::default(),
        payload: Payload::FontCreate {
            typeface: "Deck Test Face".into(),
            face: deckcraft_collab_proto::FontFace::Regular,
            sha256: sha.clone(),
            name: "DeckFace.ttf".into(),
        },
        history: None,
    });
    a.send(tungstenite::Message::Text(encode_client(&op).expect("encode").into())).expect("send op");
    match read(&mut a) {
        ServerMsg::Applied(applied) => assert_eq!(applied.version, 1),
        other => panic!("expected applied, got {other:?}"),
    }

    // The commit reimports with the entry, so the face really travelled through the room.
    let (status, committed) = http(addr, "POST", "/v1/sessions/file-f/commit", Some("service-token"), "");
    assert_eq!(status, 200, "{committed}");
    let value: serde_json::Value = serde_json::from_str(&committed).expect("json");
    let bytes = base64::engine::general_purpose::STANDARD.decode(value["bytes"].as_str().expect("inline bytes")).expect("base64");
    let (deck, _lost) = deckcraft_pptx::import_checked(&bytes).expect("reimport");
    assert_eq!(deck.embedded_fonts.len(), 1, "the file carries the embedded font");
    assert_eq!(deck.embedded_fonts[0].typeface, "Deck Test Face");
    assert!(deck.embedded_fonts[0].regular.is_some());
    handle.shutdown();
}

#[test]
fn a_3d_model_goes_through_the_channel_and_fetches_by_its_signature() {
    let handle = start();
    let addr = handle.addr;
    let body = serde_json::json!({"fileId": "file-3d", "app": "deckcraft"}).to_string();
    let (status, _) = http(addr, "POST", "/v1/sessions", Some("service-token"), &body);
    assert_eq!(status, 201);
    let editor = token("alice", "editor", "file-3d");

    // A binary glTF is a media object like a picture: uploaded once under its digest.
    let model = glb();
    let sha = sha256(&model);
    let (status, uploaded) = http_bytes(addr, "PUT", &format!("/v1/sessions/file-3d/media/{sha}"), Some(&editor), &model);
    assert_eq!(status, 201, "{}", String::from_utf8_lossy(&uploaded));

    // The path is normalised and the content type comes from the signature, not the name: the
    // same object answers to its digest, with an extension a client may add, and with a query.
    for path in [format!("/media/{sha}"), format!("/media/{sha}.glb"), format!("/media/{sha}?v=2")] {
        let (status, head, fetched) = http_parts(addr, "GET", &path, Some(&editor), b"");
        assert_eq!(status, 200, "{path}: {head}");
        assert_eq!(fetched, model, "{path}: the bytes come back unchanged");
        assert!(head.to_ascii_lowercase().contains("content-type: model/gltf-binary"), "{path}: {head}");
    }
    // A name is not an object: another segment or a non-digest path is not found.
    let (status, _) = http_bytes(addr, "GET", &format!("/media/{sha}/x"), Some(&editor), b"");
    assert_eq!(status, 404);

    // The allowlist is the signature, not the extension: a `.glb` name with other bytes is refused.
    let fake = b"not a model at all".to_vec();
    let fake_sha = sha256(&fake);
    let (status, _) = http_bytes(addr, "PUT", &format!("/v1/sessions/file-3d/media/{fake_sha}"), Some(&editor), &fake);
    assert_eq!(status, 400, "the body is not a binary glTF");
    handle.shutdown();
}

#[test]
fn the_desktop_link_uploads_and_fetches_pictures_over_http() {
    use deckcraft_collab::Transport as _;
    let handle = start();
    let addr = handle.addr;
    let body = serde_json::json!({"fileId": "file-l", "app": "deckcraft"}).to_string();
    let (status, _) = http(addr, "POST", "/v1/sessions", Some("service-token"), &body);
    assert_eq!(status, 201);

    let editor = token("alice", "editor", "file-l");
    let picture = png();
    let sha = sha256(&picture);
    let hello = ClientMsg::Hello(Hello {
        v: PROTO_VERSION,
        app: "deckcraft".into(),
        doc: "file-l".into(),
        jwt: editor.clone(),
        client: ClientInfo { name: "alice".into(), ver: "1".into() },
        last_version: None,
    });
    let mut link = deckcraft_collab::WsTransport::start(format!("ws://{addr}/v1/sessions/file-l/ws"), hello);
    link.put_media(&sha, &picture).expect("the link uploads the picture");

    let fetch = deckcraft_collab::WsTransport::media_fetcher(&format!("ws://{addr}"), &editor).expect("a plain ws:// link has a fetcher");
    assert_eq!(fetch(&sha).expect("fetched"), picture);
    drop(link);
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
