//! DeckCraft collaboration server.
//!
//! The process around [`deckcraft_collab::Hub`]: HTTP session control, a WebSocket per
//! participant, JWT authentication in the first frame, limits, health and metrics.
//!
//! What it deliberately does not do yet: a lease table and a browser transport. The media
//! channel carries pictures, audio and video by `sha256` (separate allowlists and byte budgets).
//! The room model and the wire types are in `deckcraft-collab`; this crate is the transport
//! around them.
//!
//! Security rules (ADR-0008, RFC §12):
//! - the JWT travels in the first WebSocket frame, never in the URL;
//! - the actor and the role come from the token; a message field cannot choose them;
//! - the cabinet's service token guards session control;
//! - logs name sessions and counts, never slide content, text or tokens.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use deckcraft_collab::session::SessionConfig;
use deckcraft_collab::session::{CommitError, Committed};
use deckcraft_collab::{Claims, Hub, media_type, sha256_hex, valid_sha};
use deckcraft_collab_proto::{
    ClientMsg, CommitResult, PROTO_VERSION, RejectCode, Rejected, Role, ServerError, ServerMsg, decode_client, encode_server,
};
use deckcraft_model::Presentation;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tungstenite::error::Error as WsError;

use crate::cabinet::{Cabinet, PublishError};

pub mod cabinet;
pub mod store;
use store::Store;

/// How a token is checked. The cabinet signs with an asymmetric key; local runs may use HS256.
#[derive(Clone)]
pub enum JwtConfig {
    Secret(Vec<u8>),
    Pem(Vec<u8>),
}

impl JwtConfig {
    fn key_and_algorithms(&self) -> Result<(DecodingKey, Vec<Algorithm>), String> {
        match self {
            JwtConfig::Secret(secret) => Ok((DecodingKey::from_secret(secret), vec![Algorithm::HS256, Algorithm::HS384, Algorithm::HS512])),
            JwtConfig::Pem(pem) => {
                if let Ok(key) = DecodingKey::from_ec_pem(pem) {
                    return Ok((key, vec![Algorithm::ES256, Algorithm::ES384]));
                }
                if let Ok(key) = DecodingKey::from_rsa_pem(pem) {
                    return Ok((key, vec![Algorithm::RS256, Algorithm::RS384, Algorithm::RS512]));
                }
                if let Ok(key) = DecodingKey::from_ed_pem(pem) {
                    return Ok((key, vec![Algorithm::EdDSA]));
                }
                Err("the PEM key is not EC, RSA or Ed25519".into())
            }
        }
    }
}

/// Server configuration, from the environment in `main`.
#[derive(Clone)]
pub struct Config {
    /// The cabinet's service token for `/v1/sessions`. `None` refuses session control.
    pub service_token: Option<String>,
    pub jwt: JwtConfig,
    pub audience: String,
    /// Most participants in one room (`0` = unlimited).
    pub max_participants: usize,
    /// Largest client frame.
    pub max_message_bytes: usize,
    /// Operations per second per connection (`0` = unlimited).
    pub max_ops_per_sec: usize,
    /// Where rooms are persisted (native base + journal). `None` keeps them in memory.
    pub store_dir: Option<PathBuf>,
    /// Operations between base snapshots.
    pub snapshot_every: u64,
    /// Largest native base sent inline to a client whose journal tail was compacted. Above it the
    /// base travels in chunks of `max_base_chunk` up to `max_base_chunks` deep.
    pub max_base_inline: usize,
    /// Bytes of one chunk of a base that is too large to inline.
    pub max_base_chunk: usize,
    /// Deepest chain of chunks for one base. A base larger than
    /// `max_base_chunk * max_base_chunks` still travels as one HTTP request
    /// (`GET /v1/sessions/{id}/base/{sha256}`); only a base above the protocol's HTTP ceiling is
    /// named with no way to fetch it, and then the client reopens the file from the cabinet.
    pub max_base_chunks: usize,
    /// Where a commit becomes a cabinet file version. `None` keeps commits inside the process.
    pub cabinet: Option<cabinet::CabinetConfig>,
    /// Origins a browser page may call this server from (`DECKCRAFT_CORS_ORIGINS`, comma
    /// separated). Empty means no CORS at all: no `Access-Control-*` header and no `Vary`, as
    /// before the media channel got a browser link. An entry is an exact origin
    /// (`https://cabinet.example`), compared case-insensitively and without a trailing slash;
    /// `*` is not special, and `Origin: null` must be listed to be admitted.
    pub cors_origins: Vec<String>,
}

impl Config {
    pub fn for_tests(secret: &[u8], service: &str) -> Self {
        Config {
            service_token: Some(service.to_string()),
            jwt: JwtConfig::Secret(secret.to_vec()),
            audience: "office-sync".into(),
            max_participants: 16,
            max_message_bytes: 256 * 1024,
            max_ops_per_sec: 50,
            store_dir: None,
            snapshot_every: 200,
            max_base_inline: 8 << 20,
            max_base_chunk: 1 << 20,
            max_base_chunks: 1024,
            cabinet: None,
            cors_origins: Vec::new(),
        }
    }
}

/// Split `DECKCRAFT_CORS_ORIGINS` into entries: comma separated, trimmed, without a trailing
/// slash, duplicates dropped, empty entries ignored. An empty value is the no-CORS mode.
pub fn parse_cors_origins(value: &str) -> Vec<String> {
    let mut origins: Vec<String> = Vec::new();
    for entry in value.split(',') {
        let origin = entry.trim().trim_end_matches('/');
        if !origin.is_empty() && !origins.iter().any(|known| same_origin(known, origin)) {
            origins.push(origin.to_string());
        }
    }
    origins
}

/// The token's claims. The role is a string here; [`Role`] is the checked value.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenClaims {
    pub sub: String,
    pub role: String,
    #[serde(default)]
    pub file_id: Option<String>,
    #[serde(default)]
    pub app: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub aud: Option<String>,
    #[serde(default)]
    pub exp: Option<u64>,
}

fn role_of(s: &str) -> Option<Role> {
    match s {
        "owner" => Some(Role::Owner),
        "editor" => Some(Role::Editor),
        "viewer" => Some(Role::Viewer),
        _ => None,
    }
}

#[derive(Default)]
struct Metrics {
    sessions: AtomicU64,
    connections: AtomicU64,
    active_connections: AtomicU64,
    messages: AtomicU64,
    ops: AtomicU64,
    rejected: AtomicU64,
    commits: AtomicU64,
    resyncs: AtomicU64,
    media_uploads: AtomicU64,
    media_bytes: AtomicU64,
    base_transfers: AtomicU64,
    base_bytes: AtomicU64,
    base_chunks: AtomicU64,
    base_http: AtomicU64,
    base_http_bytes: AtomicU64,
    cabinet_commits: AtomicU64,
    cabinet_conflicts: AtomicU64,
    cabinet_failures: AtomicU64,
    bytes_in: AtomicU64,
    bytes_out: AtomicU64,
}

impl Metrics {
    fn render(&self, rooms: usize) -> String {
        let mut out = String::new();
        let row = |name: &str, value: u64| format!("deckcraft_collab_{name} {value}\n");
        out.push_str(&row("rooms", rooms as u64));
        out.push_str(&row("sessions_total", self.sessions.load(Ordering::Relaxed)));
        out.push_str(&row("connections_total", self.connections.load(Ordering::Relaxed)));
        out.push_str(&row("connections_active", self.active_connections.load(Ordering::Relaxed)));
        out.push_str(&row("messages_total", self.messages.load(Ordering::Relaxed)));
        out.push_str(&row("ops_total", self.ops.load(Ordering::Relaxed)));
        out.push_str(&row("rejected_total", self.rejected.load(Ordering::Relaxed)));
        out.push_str(&row("commits_total", self.commits.load(Ordering::Relaxed)));
        out.push_str(&row("resyncs_total", self.resyncs.load(Ordering::Relaxed)));
        out.push_str(&row("media_uploads_total", self.media_uploads.load(Ordering::Relaxed)));
        out.push_str(&row("media_bytes_total", self.media_bytes.load(Ordering::Relaxed)));
        out.push_str(&row("base_transfers_total", self.base_transfers.load(Ordering::Relaxed)));
        out.push_str(&row("base_bytes_total", self.base_bytes.load(Ordering::Relaxed)));
        out.push_str(&row("base_chunks_total", self.base_chunks.load(Ordering::Relaxed)));
        out.push_str(&row("base_http_total", self.base_http.load(Ordering::Relaxed)));
        out.push_str(&row("base_http_bytes_total", self.base_http_bytes.load(Ordering::Relaxed)));
        out.push_str(&row("cabinet_commits_total", self.cabinet_commits.load(Ordering::Relaxed)));
        out.push_str(&row("cabinet_conflicts_total", self.cabinet_conflicts.load(Ordering::Relaxed)));
        out.push_str(&row("cabinet_failures_total", self.cabinet_failures.load(Ordering::Relaxed)));
        out.push_str(&row("bytes_in_total", self.bytes_in.load(Ordering::Relaxed)));
        out.push_str(&row("bytes_out_total", self.bytes_out.load(Ordering::Relaxed)));
        out
    }
}

struct Shared {
    config: Config,
    hubs: Mutex<HashMap<String, Arc<Mutex<Hub>>>>,
    /// The display name each room was opened with, for the cabinet callback.
    names: Mutex<HashMap<String, String>>,
    metrics: Metrics,
    store: Option<Store>,
    /// The cabinet's commit endpoint; `None` keeps commits inside the process.
    cabinet: Option<Cabinet>,
}

/// A running server. Dropping it stops the accept loop.
pub struct Handle {
    pub addr: SocketAddr,
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl Handle {
    pub fn shutdown(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Bind `addr` and serve until the handle is shut down.
pub fn serve(addr: SocketAddr, config: Config) -> std::io::Result<Handle> {
    let listener = TcpListener::bind(addr)?;
    let addr = listener.local_addr()?;
    listener.set_nonblocking(true)?;
    let store = match &config.store_dir {
        Some(dir) => match Store::new(dir, config.snapshot_every) {
            Ok(store) => Some(store),
            Err(e) => return Err(std::io::Error::other(format!("store {}: {e}", dir.display()))),
        },
        None => None,
    };
    let shared = Arc::new(Shared {
        cabinet: config.cabinet.clone().map(Cabinet::new),
        config,
        hubs: Mutex::new(HashMap::new()),
        names: Mutex::new(HashMap::new()),
        metrics: Metrics::default(),
        store,
    });
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = stop.clone();
    let join = thread::spawn(move || {
        while !stop_flag.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let shared = shared.clone();
                    thread::spawn(move || {
                        if let Err(e) = handle_connection(stream, shared) {
                            log::debug!("connection closed: {e}");
                        }
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(20)),
                Err(e) => {
                    log::warn!("accept failed: {e}");
                    thread::sleep(Duration::from_millis(200));
                }
            }
        }
    });
    Ok(Handle { addr, stop, join: Some(join) })
}

/// Serve on an ephemeral port (tests).
pub fn serve_ephemeral(config: Config) -> std::io::Result<Handle> {
    serve("127.0.0.1:0".parse().unwrap_or_else(|_| SocketAddr::from(([127, 0, 0, 1], 0))), config)
}

fn handle_connection(mut stream: TcpStream, shared: Arc<Shared>) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_nodelay(true)?;
    let mut probe = [0u8; 4096];
    let n = stream.peek(&mut probe)?;
    let head = String::from_utf8_lossy(&probe[..n]).to_ascii_lowercase();
    if head.contains("upgrade: websocket") {
        return websocket(stream, shared);
    }
    let request = read_request(&mut stream)?;
    let response = route(&request, &shared)?;
    write_response(&mut stream, response)
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    fn bearer(&self) -> Option<&str> {
        self.header("authorization")?.strip_prefix("Bearer ")
    }
}

struct Response {
    status: u16,
    reason: &'static str,
    content_type: &'static str,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

impl Response {
    fn json(status: u16, value: Value) -> Self {
        let reason = match status {
            200 => "OK",
            201 => "Created",
            204 => "No Content",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            409 => "Conflict",
            413 => "Payload Too Large",
            502 => "Bad Gateway",
            _ => "Internal Server Error",
        };
        Response { status, reason, content_type: "application/json", headers: Vec::new(), body: value.to_string().into_bytes() }
    }
    fn text(status: u16, reason: &'static str, body: String) -> Self {
        Response { status, reason, content_type: "text/plain; version=0.0.4", headers: Vec::new(), body: body.into_bytes() }
    }
    fn binary(status: u16, content_type: &'static str, body: Vec<u8>) -> Self {
        Response { status, reason: "OK", content_type, headers: Vec::new(), body }
    }
    /// An empty answer with no body type (a CORS preflight).
    fn empty(status: u16, reason: &'static str) -> Self {
        Response { status, reason, content_type: "text/plain", headers: Vec::new(), body: Vec::new() }
    }
    fn add_header(&mut self, name: &'static str, value: impl Into<String>) {
        self.headers.push((name, value.into()));
    }
}

/// Connection ids are per socket, never the actor: a person may have a second tab or reconnect
/// before the server noticed the old socket close, and the two must not share one outbox.
static NEXT_PEER: AtomicU64 = AtomicU64::new(1);

/// The largest request body any HTTP endpoint accepts: the media channel's biggest single object
/// (audio/video and 3-D models are 32 MiB; a picture is 8, a font face 16). A media `PUT` carries
/// the raw object, so the reader must be at least as large as the widest kind.
const MAX_REQUEST_BODY: usize = if deckcraft_collab::MAX_AV_BYTES > deckcraft_collab::MAX_MODEL_BYTES {
    deckcraft_collab::MAX_AV_BYTES
} else {
    deckcraft_collab::MAX_MODEL_BYTES
};

fn read_request(stream: &mut TcpStream) -> std::io::Result<Request> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let header_end = loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "empty request"));
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find(&buf, b"\r\n\r\n") {
            break pos + 4;
        }
        if buf.len() > 64 * 1024 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "request headers too large"));
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.lines();
    let first = lines.next().unwrap_or_default();
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or("/").split('?').next().unwrap_or("/").to_string();
    let mut headers = Vec::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let length = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.parse::<usize>().ok()).unwrap_or(0);
    // The body of a `PUT /v1/sessions/{id}/media/{sha256}` is the media object itself, and the
    // media channel accepts up to the largest kind's budget. A smaller cap here would refuse an
    // upload that the endpoint itself is ready to take (an audio or video clip, a 3-D model).
    if length > MAX_REQUEST_BODY {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "body too large"));
    }
    let mut body = buf[header_end..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(length);
    Ok(Request { method, path, headers, body })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn write_response(stream: &mut TcpStream, response: Response) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        response.reason,
        response.content_type,
        response.body.len()
    );
    for (name, value) in &response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()
}

fn route(request: &Request, shared: &Arc<Shared>) -> std::io::Result<Response> {
    if shared.config.cors_origins.is_empty() {
        return route_inner(request, shared);
    }
    if request.method == "OPTIONS" && request.header("origin").is_some() {
        return Ok(preflight(request, shared));
    }
    let mut response = route_inner(request, shared)?;
    apply_cors(&mut response, request, shared);
    Ok(response)
}

/// The origins an entry admits, compared case-insensitively and without a trailing slash (the
/// path part is never part of an `Origin`).
fn same_origin(a: &str, b: &str) -> bool {
    let normalise = |value: &str| value.trim().trim_end_matches('/').to_ascii_lowercase();
    normalise(a) == normalise(b)
}

/// The request's `Origin` if the allowlist admits it; `None` for a foreign origin, a request
/// without one, or an empty allowlist.
fn allowed_origin<'a>(request: &'a Request, shared: &Shared) -> Option<&'a str> {
    if shared.config.cors_origins.is_empty() {
        return None;
    }
    let origin = request.header("origin")?;
    shared.config.cors_origins.iter().any(|allowed| same_origin(allowed, origin)).then_some(origin)
}

/// Headers for one ordinary (non-preflight) response: `Vary: Origin` always while CORS is
/// configured — the answer depends on the origin even when the origin is refused — and
/// `Access-Control-Allow-Origin` only for an admitted origin, never a wildcard.
fn apply_cors(response: &mut Response, request: &Request, shared: &Shared) {
    if shared.config.cors_origins.is_empty() {
        return;
    }
    response.add_header("Vary", "Origin");
    if let Some(origin) = allowed_origin(request, shared) {
        response.add_header("Access-Control-Allow-Origin", origin);
    }
}

/// Methods a browser page may use against this server: the media channel's `GET`/`PUT`, session
/// control's `POST` (and the `OPTIONS` the preflight itself is).
const CORS_ALLOW_METHODS: &str = "GET, POST, PUT, OPTIONS";
/// The only request headers the browser link sends: the session token and a body type.
const CORS_ALLOW_HEADERS: &str = "Authorization, Content-Type";

/// `OPTIONS` preflight. The browser sends no token here (a preflight carries no
/// `Authorization`), so this answer names the methods and headers and nothing else — it reveals
/// no room, media object or version. A foreign origin gets `Vary: Origin` and no
/// `Access-Control-*` header, so the browser refuses the real request. A method outside the
/// allowlist gets `405` with no `Access-Control-*` header either.
fn preflight(request: &Request, shared: &Shared) -> Response {
    let mut response = Response::empty(204, "No Content");
    response.add_header("Vary", "Origin");
    let asked = request.header("access-control-request-method").unwrap_or("GET");
    if !CORS_ALLOW_METHODS.split(", ").any(|allowed| allowed.eq_ignore_ascii_case(asked)) {
        let mut refusal = Response::empty(405, "Method Not Allowed");
        refusal.add_header("Vary", "Origin");
        return refusal;
    }
    if let Some(origin) = allowed_origin(request, shared) {
        response.add_header("Access-Control-Allow-Origin", origin);
        response.add_header("Access-Control-Allow-Methods", CORS_ALLOW_METHODS);
        response.add_header("Access-Control-Allow-Headers", CORS_ALLOW_HEADERS);
        response.add_header("Access-Control-Max-Age", "600");
    }
    response
}

fn route_inner(request: &Request, shared: &Arc<Shared>) -> std::io::Result<Response> {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/healthz") | ("GET", "/readyz") => Ok(Response::json(200, json!({"ok": true, "version": env!("CARGO_PKG_VERSION")}))),
        ("GET", "/metrics") => {
            let rooms = shared.hubs.lock().map(|h| h.len()).unwrap_or(0);
            Ok(Response::text(200, "OK", shared.metrics.render(rooms)))
        }
        ("POST", "/v1/sessions") => create_session(request, shared),
        ("GET", path) if path.starts_with("/media/") => Ok(get_media(&path["/media/".len()..], request, shared)),
        (method, path) if path.starts_with("/v1/sessions/") => {
            let rest = &path["/v1/sessions/".len()..];
            let parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
            match (method, parts.as_slice()) {
                ("GET", [id]) => session_info(id, shared, request),
                ("DELETE", [id]) => close_session(id, shared, request),
                ("POST", [id, "commit"]) => commit_session(id, request, shared),
                ("GET", [id, "base", sha]) => Ok(get_base(id, sha, shared, request)),
                ("PUT", [id, "media", sha]) => put_media(id, sha, request, shared),
                _ => Ok(Response::json(if parts.len() > 2 { 404 } else { 405 }, json!({"error": "not found"}))),
            }
        }
        _ => Ok(Response::json(404, json!({"error": "not found"}))),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateBody {
    file_id: String,
    #[serde(default)]
    app: Option<String>,
    #[serde(default)]
    initial_bytes: Option<String>,
    #[serde(default)]
    initial_name: Option<String>,
    #[serde(default)]
    expected_version: Option<String>,
}

fn service_allowed(request: &Request, shared: &Shared) -> bool {
    let Some(expected) = &shared.config.service_token else { return false };
    request.bearer().is_some_and(|token| token == expected)
}

fn create_session(request: &Request, shared: &Arc<Shared>) -> std::io::Result<Response> {
    if !service_allowed(request, shared) {
        return Ok(Response::json(401, json!({"error": "service token required"})));
    }
    let Ok(body) = serde_json::from_slice::<CreateBody>(&request.body) else {
        return Ok(Response::json(400, json!({"error": "invalid JSON"})));
    };
    if body.app.as_deref().is_some_and(|app| app != "deckcraft") {
        return Ok(Response::json(400, json!({"error": "this server serves deckcraft only"})));
    }
    if body.file_id.is_empty() || body.file_id.len() > 256 {
        return Ok(Response::json(400, json!({"error": "fileId is empty or too long"})));
    }
    let mut hubs = shared.hubs.lock().map_err(|_| std::io::Error::other("hub lock"))?;
    if hubs.contains_key(&body.file_id) {
        drop(hubs);
        return Ok(Response::json(200, session_json(&body.file_id, shared)));
    }
    let stored = match &shared.store {
        Some(store) => match store.load(&body.file_id) {
            Ok(session) => session,
            Err(e) => return Ok(Response::json(500, json!({"error": e}))),
        },
        None => None,
    };
    let mut session = match stored {
        Some(session) => session,
        None => {
            let session = match body.initial_bytes {
                Some(ref encoded) => {
                    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
                        return Ok(Response::json(400, json!({"error": "initialBytes is not base64"})));
                    };
                    deckcraft_collab::DocSession::open(&body.file_id, &bytes)
                }
                None => deckcraft_collab::DocSession::from_document(&body.file_id, Presentation::default(), SessionConfig::default()),
            };
            match session {
                Ok(session) => session,
                Err(e) => return Ok(Response::json(400, json!({"error": e}))),
            }
        }
    };
    if let Some(version) = body.expected_version {
        session.set_base_file_etag(version);
    }
    // The server's snapshot frequency applies to a memory-only room as well as to the store, so a
    // deployment can force compaction (and a base transfer) with one setting.
    session.set_config(SessionConfig {
        snapshot_every: shared.config.snapshot_every,
        max_base_inline: shared.config.max_base_inline,
        max_base_chunk: shared.config.max_base_chunk,
        max_base_chunks: shared.config.max_base_chunks,
        ..SessionConfig::default()
    });
    if let Some(name) = body.initial_name.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
        if name.len() > 255 || name.chars().any(char::is_control) {
            return Ok(Response::json(400, json!({"error": "initialName is not a display name"})));
        }
        if let Ok(mut names) = shared.names.lock() {
            names.entry(body.file_id.clone()).or_insert_with(|| name.to_string());
        }
    }
    if let Some(store) = &shared.store
        && let Err(e) = store.snapshot(&body.file_id, &mut session)
    {
        return Ok(Response::json(500, json!({"error": e})));
    }
    let hub = Hub::new(session);
    hubs.insert(body.file_id.clone(), Arc::new(Mutex::new(hub)));
    drop(hubs);
    shared.metrics.sessions.fetch_add(1, Ordering::Relaxed);
    Ok(Response::json(201, session_json(&body.file_id, shared)))
}

fn session_json(id: &str, shared: &Shared) -> Value {
    let (participants, version) = shared
        .hubs
        .lock()
        .ok()
        .and_then(|h| h.get(id).cloned())
        .and_then(|hub| hub.lock().ok().map(|hub| (hub.participants().len(), hub.version())))
        .unwrap_or((0, 0));
    json!({
        "sessionId": id,
        "fileId": id,
        "state": "open",
        "participants": participants,
        "version": version,
        "wsUrl": format!("/v1/sessions/{id}/ws"),
        "expiresAt": now_secs().saturating_add(3600),
    })
}

fn session_info(id: &str, shared: &Arc<Shared>, _request: &Request) -> std::io::Result<Response> {
    let hubs = shared.hubs.lock().map_err(|_| std::io::Error::other("hub lock"))?;
    if !hubs.contains_key(id) {
        return Ok(Response::json(404, json!({"error": "no such session"})));
    }
    drop(hubs);
    Ok(Response::json(200, session_json(id, shared)))
}

fn close_session(id: &str, shared: &Arc<Shared>, request: &Request) -> std::io::Result<Response> {
    if !service_allowed(request, shared) {
        return Ok(Response::json(401, json!({"error": "service token required"})));
    }
    let mut hubs = shared.hubs.lock().map_err(|_| std::io::Error::other("hub lock"))?;
    let Some(hub) = hubs.get(id).cloned() else {
        return Ok(Response::json(404, json!({"error": "no such session"})));
    };
    // The live document is captured while the room is still in the map and still serving. The
    // journal alone is not a save: without a base at the head, a store whose journal is damaged
    // afterwards can only be reopened from the cabinet's older bytes, and every accepted step
    // since the last base is gone. A snapshot that fails is therefore a failed close: the room
    // stays where it is — still answering its sockets and still the one a `commit` binds to — so
    // the operator can retry instead of losing the only live copy of the document.
    let Ok(mut hub) = hub.lock() else {
        return Ok(Response::json(500, json!({"error": "the room could not be closed safely", "closed": false})));
    };
    if let Some(store) = &shared.store
        && let Err(e) = store.snapshot(id, hub.session_mut())
    {
        return Ok(Response::json(500, json!({"error": format!("the room could not be snapshotted: {e}"), "closed": false})));
    }
    // The sockets still hold this `Arc`: mark the room closed first so their next message
    // is refused and their pump exits, before a re-created session can load the same
    // store directory and append to one journal from two in-memory rooms.
    hub.close();
    drop(hub);
    hubs.remove(id);
    Ok(Response::json(200, json!({"closed": true})))
}

/// `POST /v1/sessions/{id}/commit`: build the `.pptx` from the authoritative document and give
/// it to the cabinet, which creates the new file version. The service token guards the call; the
/// cabinet answers the version it stored, or `409` when the file changed outside the session.
///
/// Without a configured callback the bytes come back inline, as `INTEGRATION.md` §4.1 describes.
fn commit_session(id: &str, request: &Request, shared: &Arc<Shared>) -> std::io::Result<Response> {
    if !service_allowed(request, shared) {
        return Ok(Response::json(401, json!({"error": "service token required"})));
    }
    let body = if request.body.is_empty() {
        CommitBody::default()
    } else {
        match serde_json::from_slice::<CommitBody>(&request.body) {
            Ok(body) => body,
            Err(_) => return Ok(Response::json(400, json!({"error": "invalid JSON"}))),
        }
    };
    shared.metrics.commits.fetch_add(1, Ordering::Relaxed);
    match build_and_publish(shared, id, body.expected_version.as_deref()) {
        Ok(done) => {
            let mut value = json!({
                "fileId": id,
                "version": done.committed.version,
                "sha256": done.sha256,
                "fileVersion": done.etag.clone().or_else(|| done.committed.file_version.clone()),
                "warnings": done.committed.warnings,
            });
            if let Some(name) = &done.name {
                value["name"] = json!(name);
            }
            if shared.cabinet.is_none() {
                value["bytes"] = json!(base64::engine::general_purpose::STANDARD.encode(&done.committed.bytes));
            }
            Ok(Response::json(200, value))
        }
        Err(CommitFailure::NotFound) => Ok(Response::json(404, json!({"error": "no such session"}))),
        Err(CommitFailure::Conflict(message)) => Ok(Response::json(409, json!({"error": message}))),
        Err(CommitFailure::Build(message)) => Ok(Response::json(500, json!({"error": message}))),
        Err(CommitFailure::Publish(message)) => Ok(Response::json(502, json!({"error": message}))),
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CommitBody {
    /// The file version the caller believes the session last saw (ADR-0009 §4).
    #[serde(default)]
    expected_version: Option<String>,
}

/// One built file, answered with the version the cabinet now holds.
struct CommittedFile {
    name: Option<String>,
    sha256: String,
    committed: Committed,
    /// The cabinet's file version after it took the bytes; `None` without a callback.
    etag: Option<String>,
}

/// Why a commit did not produce a cabinet version.
enum CommitFailure {
    NotFound,
    Conflict(String),
    Build(String),
    Publish(String),
}

/// Build the file from the room's authoritative state, then post it to the cabinet. The room lock
/// is released before the callback, so one slow cabinet cannot stall the other participants.
fn build_and_publish(shared: &Arc<Shared>, file_id: &str, expected: Option<&str>) -> Result<CommittedFile, CommitFailure> {
    let hub = live_hub(shared, file_id)?;
    build_and_publish_on(&hub, shared, file_id, expected)
}

/// The room the server serves under `file_id`, or `NotFound`.
fn live_hub(shared: &Arc<Shared>, file_id: &str) -> Result<Arc<Mutex<Hub>>, CommitFailure> {
    shared.hubs.lock().map_err(|_| CommitFailure::Publish("the room lock is poisoned".into()))?.get(file_id).cloned().ok_or(CommitFailure::NotFound)
}

/// Whether `hub` is still the live room for `file_id`. `DELETE` closes a room before it leaves the
/// map, so a socket that outlived its room must neither build nor publish the document that
/// replaced it: the authorization and the commit are bound to one hub instance.
fn still_live(shared: &Arc<Shared>, file_id: &str, hub: &Arc<Mutex<Hub>>) -> bool {
    let in_map = shared.hubs.lock().ok().is_some_and(|hubs| hubs.get(file_id).is_some_and(|live| Arc::ptr_eq(live, hub)));
    in_map && hub.lock().map(|hub| !hub.is_closed()).unwrap_or(false)
}

/// Build and publish against one hub instance (the one the caller was authorized against).
fn build_and_publish_on(hub: &Arc<Mutex<Hub>>, shared: &Arc<Shared>, file_id: &str, expected: Option<&str>) -> Result<CommittedFile, CommitFailure> {
    if !still_live(shared, file_id, hub) {
        return Err(CommitFailure::NotFound);
    }
    let (committed, expected_version) = {
        let mut hub = hub.lock().map_err(|_| CommitFailure::Publish("the room lock is poisoned".into()))?;
        let committed = hub.session_mut().commit_checked(expected).map_err(|error| match error {
            CommitError::Conflict { expected, current } => CommitFailure::Conflict(match current {
                Some(current) => format!("the file changed outside the session: expected {expected}, current {current}"),
                None => format!("the file changed outside the session: expected {expected}"),
            }),
            CommitError::Build(message) => CommitFailure::Build(message),
        })?;
        let expected_version = hub.session().base_file_etag().map(str::to_string);
        (committed, expected_version)
    };
    let sha256 = sha256_hex(&committed.bytes);
    let name = shared.names.lock().ok().and_then(|names| names.get(file_id).cloned());
    let published = match &shared.cabinet {
        Some(cabinet) => match cabinet.publish(file_id, name.as_deref(), committed.version, expected_version.as_deref(), &committed.bytes, &sha256) {
            Ok(published) => Some(published),
            Err(PublishError::Conflict(message)) => {
                shared.metrics.cabinet_conflicts.fetch_add(1, Ordering::Relaxed);
                return Err(CommitFailure::Conflict(message));
            }
            Err(PublishError::Failed(message)) => {
                shared.metrics.cabinet_failures.fetch_add(1, Ordering::Relaxed);
                return Err(CommitFailure::Publish(message));
            }
        },
        None => None,
    };
    let etag = published.as_ref().map(|published| published.etag.clone());
    if let Some(etag) = &etag {
        if still_live(shared, file_id, hub) {
            hub.lock().map_err(|_| CommitFailure::Publish("the room lock is poisoned".into()))?.accept_file_version(etag.clone());
            if let Some(store) = &shared.store
                && let Err(e) = store.set_file_version(file_id, etag)
            {
                log::error!("file version not stored: {e}");
            }
            shared.metrics.cabinet_commits.fetch_add(1, Ordering::Relaxed);
        } else {
            // The room was closed (and possibly re-created) while the cabinet took the file. The
            // version belongs to the session the caller was authorized against; that session is
            // gone, so nothing is adopted by whatever now serves the same file id.
            log::warn!("commit {file_id}: the room closed before the cabinet's answer");
        }
    }
    Ok(CommittedFile { name, sha256, committed, etag })
}

fn commit_result(done: &CommittedFile) -> CommitResult {
    CommitResult {
        sha256: done.sha256.clone(),
        version: done.committed.version,
        file_version: done.etag.clone().or_else(|| done.committed.file_version.clone()),
        bytes_base64: Some(base64::engine::general_purpose::STANDARD.encode(&done.committed.bytes)),
        blob: None,
        warnings: done.committed.warnings.clone(),
    }
}

/// A commit requested by a peer. The answer is queued only after the cabinet took the file, so a
/// `commit-result` never promises a version that does not exist.
fn commit_from_client(hub: &Arc<Mutex<Hub>>, peer: &str, expected: Option<&str>, shared: &Arc<Shared>) -> std::io::Result<()> {
    shared.metrics.commits.fetch_add(1, Ordering::Relaxed);
    let (file_id, allowed) = {
        let hub = hub.lock().map_err(|_| std::io::Error::other("hub lock"))?;
        (hub.file_id().to_string(), hub.may_commit(peer))
    };
    if !allowed {
        return answer(hub, peer, ServerMsg::Rejected(rejected("commit", RejectCode::Forbidden, "a viewer cannot commit")));
    }
    match build_and_publish_on(hub, shared, &file_id, expected) {
        Ok(done) => answer(hub, peer, ServerMsg::CommitResult(commit_result(&done))),
        Err(CommitFailure::Conflict(message)) => answer(hub, peer, ServerMsg::Rejected(rejected("commit", RejectCode::Conflict, &message))),
        Err(CommitFailure::Build(message)) => answer(hub, peer, ServerMsg::Error(ServerError { code: "commit".into(), message, retryable: true })),
        Err(CommitFailure::Publish(message)) => {
            answer(hub, peer, ServerMsg::Error(ServerError { code: "commit-publish".into(), message, retryable: true }))
        }
        Err(CommitFailure::NotFound) => {
            answer(hub, peer, ServerMsg::Error(ServerError { code: "not-found".into(), message: "no such session".into(), retryable: false }))
        }
    }
}

fn answer(hub: &Arc<Mutex<Hub>>, peer: &str, msg: ServerMsg) -> std::io::Result<()> {
    hub.lock().map_err(|_| std::io::Error::other("hub lock"))?.answer(peer, msg);
    Ok(())
}

fn rejected(op_id: &str, code: RejectCode, message: &str) -> Rejected {
    Rejected { op_id: op_id.into(), code, message: message.into(), retryable: false, hint: None }
}

/// `GET /v1/sessions/{id}/base/{sha256}`: the native base this room would send a client that
/// missed a compaction, answered in one request instead of a chain of chunks (RFC §4.5.2). The
/// pattern is the media reader's: the session token names the caller, the path names the bytes by
/// their digest, and the reply carries the native format's content type. A base the session no
/// longer holds (compaction moved on, the room is not loaded) is a `404`, so the client asks for
/// the session again rather than installing bytes the room does not have.
fn get_base(id: &str, sha: &str, shared: &Arc<Shared>, request: &Request) -> Response {
    if !valid_sha(sha) {
        return Response::json(404, json!({"error": "not found"}));
    }
    if !base_reader_allowed(id, request, shared) {
        return Response::json(401, json!({"error": "a session token for this file is required"}));
    }
    let hub = shared.hubs.lock().ok().and_then(|hubs| hubs.get(id).cloned());
    let Some(hub) = hub else {
        return Response::json(404, json!({"error": "no such session"}));
    };
    let bytes = {
        let hub = match hub.lock() {
            Ok(hub) => hub,
            Err(_) => return Response::json(500, json!({"error": "internal"})),
        };
        match hub.session().snapshot() {
            Some(snapshot) if snapshot.sha256 == sha => snapshot.bytes.clone(),
            _ => return Response::json(404, json!({"error": "the session has no such base"})),
        }
    };
    shared.metrics.base_http.fetch_add(1, Ordering::Relaxed);
    shared.metrics.base_http_bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
    Response::binary(200, "application/vnd.deckcraft.native", bytes)
}

/// Who may read a base: the same callers as the media reader, and the token must name this file
/// when it names one — the base is the whole document, not one object of it. A viewer may read it:
/// a viewer below a compacted journal still has to rebuild its replica.
fn base_reader_allowed(id: &str, request: &Request, shared: &Shared) -> bool {
    let Some(token) = request.bearer() else { return false };
    if shared.config.service_token.as_deref() == Some(token) {
        return true;
    }
    verify_token(token, &shared.config.jwt, &shared.config.audience)
        .is_ok_and(|claims| claims.app.as_deref().is_none_or(|app| app == "deckcraft") && claims.file_id.as_deref().is_none_or(|file| file == id))
}

/// `PUT /v1/sessions/{id}/media/{sha256}`: upload one media object (a picture, a font face, an
/// audio/video clip or a 3-D model). The session token decides the role and the file; the body is
/// the object, and the path must name its digest. Content-addressed, so a repeated upload of the
/// same bytes changes nothing.
fn put_media(id: &str, sha: &str, request: &Request, shared: &Arc<Shared>) -> std::io::Result<Response> {
    if !valid_sha(sha) {
        return Ok(Response::json(400, json!({"error": "the media name is not a sha256 digest"})));
    }
    let Some(token) = request.bearer() else {
        return Ok(Response::json(401, json!({"error": "a session token is required"})));
    };
    let claims = match verify_token(token, &shared.config.jwt, &shared.config.audience) {
        Ok(claims) => claims,
        Err(_) => return Ok(Response::json(401, json!({"error": "the token is not valid"}))),
    };
    if claims.app.as_deref().is_some_and(|app| app != "deckcraft") {
        return Ok(Response::json(403, json!({"error": "the token is for another app"})));
    }
    if claims.file_id.as_deref().is_some_and(|file| file != id) {
        return Ok(Response::json(403, json!({"error": "the token is for another file"})));
    }
    if !role_of(&claims.role).is_some_and(Role::may_write) {
        return Ok(Response::json(403, json!({"error": "a viewer cannot upload media"})));
    }
    if request.body.is_empty() {
        return Ok(Response::json(400, json!({"error": "the media object is empty"})));
    }
    let Some(kind) = deckcraft_collab::media_type("", &request.body) else {
        return Ok(Response::json(400, json!({"error": "the body is not a picture, font, video, audio or 3-D model file this channel stores"})));
    };
    let max = kind.max_bytes();
    if request.body.len() > max {
        return Ok(Response::json(413, json!({"error": format!("the {} is larger than {max} bytes", kind.noun())})));
    }
    if sha256_hex(&request.body) != sha {
        return Ok(Response::json(400, json!({"error": "the media bytes do not match their digest"})));
    }
    let hub = {
        let hubs = shared.hubs.lock().map_err(|_| std::io::Error::other("hub lock"))?;
        hubs.get(id).cloned()
    };
    let Some(hub) = hub else {
        return Ok(Response::json(404, json!({"error": "no such session"})));
    };
    {
        let mut hub = hub.lock().map_err(|_| std::io::Error::other("hub lock"))?;
        if let Err(e) = hub.put_media("", request.body.clone()) {
            return Ok(Response::json(400, json!({"error": e.message})));
        }
    }
    if let Some(store) = &shared.store
        && let Err(e) = store.save_media(sha, &request.body)
    {
        log::error!("media {sha}: {e}");
        return Ok(Response::json(500, json!({"error": "the media object could not be stored"})));
    }
    shared.metrics.media_uploads.fetch_add(1, Ordering::Relaxed);
    shared.metrics.media_bytes.fetch_add(request.body.len() as u64, Ordering::Relaxed);
    Ok(Response::json(201, json!({"sha256": sha, "bytes": request.body.len()})))
}

/// `GET /media/{sha256}`: one media object by digest. Content-addressed, so any authenticated
/// session may fetch it if it knows the name. The path is normalised first (a query string, a
/// trailing slash and a file extension such as `.glb` are not part of the name) and the content
/// type comes from the stored bytes' signature, never from what the caller asked for.
fn get_media(path: &str, request: &Request, shared: &Arc<Shared>) -> Response {
    let Some(sha) = media_sha(path) else {
        return Response::json(404, json!({"error": "not found"}));
    };
    if !media_reader_allowed(request, shared) {
        return Response::json(401, json!({"error": "a session or service token is required"}));
    }
    if let Some(bytes) = shared.store.as_ref().and_then(|store| store.media(sha)) {
        return media_response(bytes);
    }
    let hubs = match shared.hubs.lock() {
        Ok(hubs) => hubs,
        Err(_) => return Response::json(500, json!({"error": "internal"})),
    };
    for hub in hubs.values() {
        if let Some(bytes) = hub.lock().ok().and_then(|hub| hub.media(sha)) {
            return media_response(bytes.as_ref().clone());
        }
    }
    Response::json(404, json!({"error": "no such media object"}))
}

/// The digest a `GET /media/...` path names. A query string, a fragment, a trailing slash and a
/// file extension (`/media/{sha}.glb`) are not part of the name; anything else — another path
/// segment, a name that is not a lowercase sha256 — is not an object this server has.
fn media_sha(path: &str) -> Option<&str> {
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let path = path.strip_suffix('/').unwrap_or(path);
    if path.is_empty() || path.contains('/') {
        return None;
    }
    let name = path.split_once('.').map_or(path, |(name, _)| name);
    valid_sha(name).then_some(name)
}

fn media_reader_allowed(request: &Request, shared: &Shared) -> bool {
    let Some(token) = request.bearer() else { return false };
    if shared.config.service_token.as_deref() == Some(token) {
        return true;
    }
    verify_token(token, &shared.config.jwt, &shared.config.audience).is_ok_and(|claims| claims.app.as_deref().is_none_or(|app| app == "deckcraft"))
}

/// One stored object as an HTTP response: the content type is decided by the signature (the name
/// is a digest and says nothing about the kind), so a replay from disk and a live object answer
/// the same way, including for a model that was uploaded once and fetched much later.
fn media_response(bytes: Vec<u8>) -> Response {
    let content_type = media_type("", &bytes).map(|kind| kind.content_type(&bytes)).unwrap_or("application/octet-stream");
    Response::binary(200, content_type, bytes)
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn verify_token(token: &str, config: &JwtConfig, audience: &str) -> Result<TokenClaims, String> {
    let (key, algorithms) = config.key_and_algorithms()?;
    for algorithm in algorithms {
        let mut validation = Validation::new(algorithm);
        validation.set_audience(&[audience]);
        validation.set_required_spec_claims(&["exp", "sub", "aud"]);
        if let Ok(data) = decode::<TokenClaims>(token, &key, &validation) {
            return Ok(data.claims);
        }
    }
    Err("the token is not valid for this audience".into())
}

fn websocket(stream: TcpStream, shared: Arc<Shared>) -> std::io::Result<()> {
    let peer = stream.peer_addr().ok();
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut ws = tungstenite::accept(stream).map_err(std::io::Error::other)?;
    shared.metrics.connections.fetch_add(1, Ordering::Relaxed);
    shared.metrics.active_connections.fetch_add(1, Ordering::Relaxed);
    let result = ws_session(&mut ws, &shared);
    shared.metrics.active_connections.fetch_sub(1, Ordering::Relaxed);
    if let Err(e) = &result {
        log::debug!("ws peer {peer:?}: {e}");
    }
    let _ = ws.close(None);
    result
}

fn ws_session(ws: &mut tungstenite::WebSocket<TcpStream>, shared: &Arc<Shared>) -> std::io::Result<()> {
    let first = ws.read().map_err(std::io::Error::other)?;
    let text = match first {
        tungstenite::Message::Text(text) => text.as_str().to_string(),
        _ => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "the first frame must be hello (text)")),
    };
    shared.metrics.bytes_in.fetch_add(text.len() as u64, Ordering::Relaxed);
    let hello = match decode_client(&text) {
        Ok(ClientMsg::Hello(hello)) => hello,
        _ => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "the first frame must be hello")),
    };
    let claims = verify_token(&hello.jwt, &shared.config.jwt, &shared.config.audience).map_err(std::io::Error::other)?;
    let role = role_of(&claims.role).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::PermissionDenied, "unknown role"))?;
    if claims.app.as_deref().is_some_and(|app| app != "deckcraft") {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "the token is for another app"));
    }
    if claims.file_id.as_deref().is_some_and(|id| id != hello.doc) {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "the token is for another file"));
    }
    let hub = {
        let hubs = shared.hubs.lock().map_err(|_| std::io::Error::other("hub lock"))?;
        hubs.get(&hello.doc).cloned()
    };
    let Some(hub) = hub else {
        return send_error(ws, "not-found", "no such session", false);
    };
    let actor = claims.sub.clone();
    let peer = format!("{actor}#{}", NEXT_PEER.fetch_add(1, Ordering::Relaxed));
    {
        let mut hub = hub.lock().map_err(|_| std::io::Error::other("hub lock"))?;
        if shared.config.max_participants > 0 && hub.participants().len() >= shared.config.max_participants {
            drop(hub);
            return send_error(ws, "full", "the room is full", false);
        }
        match hub.join(&peer, &hello, Claims { actor_id: actor.clone(), role }) {
            Ok(welcome) => send_msg(ws, &ServerMsg::Welcome(welcome), shared)?,
            Err(e) => return send_error(ws, &e.code, &e.message, e.retryable),
        }
    }
    ws.get_mut().set_read_timeout(Some(Duration::from_millis(50))).map_err(std::io::Error::other)?;
    let mut ops = VecDeque::new();
    loop {
        match ws.read() {
            Ok(tungstenite::Message::Text(text)) => {
                shared.metrics.messages.fetch_add(1, Ordering::Relaxed);
                shared.metrics.bytes_in.fetch_add(text.len() as u64, Ordering::Relaxed);
                if text.len() > shared.config.max_message_bytes {
                    return send_error(ws, "too-large", "the message is too large", false);
                }
                match decode_client(text.as_str()) {
                    Ok(ClientMsg::Hello(_)) => return send_error(ws, "invalid", "already joined", false),
                    Ok(ClientMsg::Op(_)) => {
                        let now = Instant::now();
                        while ops.front().is_some_and(|t: &Instant| now.duration_since(*t) > Duration::from_secs(1)) {
                            ops.pop_front();
                        }
                        if shared.config.max_ops_per_sec > 0 && ops.len() >= shared.config.max_ops_per_sec {
                            let error = ServerError { code: "rate-limit".into(), message: "too many operations".into(), retryable: true };
                            send_msg(ws, &ServerMsg::Error(error), shared)?;
                        } else {
                            ops.push_back(now);
                            if let Ok(msg) = decode_client(text.as_str()) {
                                handle_client_message(&hub, &peer, msg, shared)?;
                            }
                        }
                    }
                    Ok(msg) => handle_client_message(&hub, &peer, msg, shared)?,
                    Err(e) => {
                        let error = ServerError { code: "invalid".into(), message: format!("bad frame: {e}"), retryable: false };
                        send_msg(ws, &ServerMsg::Error(error), shared)?;
                    }
                }
            }
            Ok(tungstenite::Message::Ping(payload)) => {
                ws.send(tungstenite::Message::Pong(payload)).map_err(std::io::Error::other)?;
            }
            Ok(tungstenite::Message::Close(_)) => break,
            Ok(_) => {}
            Err(WsError::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(WsError::ConnectionClosed | WsError::AlreadyClosed) => break,
            Err(e) => return Err(std::io::Error::other(e)),
        }
        drain(&hub, &peer, ws, shared)?;
    }
    if let Ok(mut hub) = hub.lock() {
        hub.remove_peer(&peer);
    }
    Ok(())
}

fn handle_client_message(hub: &Arc<Mutex<Hub>>, peer: &str, msg: ClientMsg, shared: &Arc<Shared>) -> std::io::Result<()> {
    if let ClientMsg::Commit { expected_file_version } = &msg {
        return commit_from_client(hub, peer, expected_file_version.as_deref(), shared);
    }
    let is_resync = matches!(msg, ClientMsg::Resync { .. });
    let is_op = matches!(msg, ClientMsg::Op(_));
    let mut hub = hub.lock().map_err(|_| std::io::Error::other("hub lock"))?;
    if let Err(e) = hub.receive(peer, msg) {
        drop(hub);
        return Err(std::io::Error::other(format!("{}: {}", e.code, e.message)));
    }
    if let Some((applied, fresh)) = hub.take_last_applied()
        && let Some(store) = &shared.store
        // Only a *new* step is journalled. A repeat of an `opId` answers the stored `Applied` and
        // changes nothing (ADR 0003): appending it again would fill the journal with duplicate
        // lines of an old version — a store whose replay still lands in the right place, but whose
        // file grows with every resend and whose versions stop being a sequence (the production
        // store of 2026-11-12 held 302 lines for a room at version 92).
        && fresh
    {
        let file_id = hub.file_id().to_string();
        if let Err(e) = store.record(&file_id, hub.session_mut(), &applied) {
            log::error!("journal {file_id} version {}: {e}", applied.version);
        }
    }
    if let Some(bytes) = hub.take_last_base_transfer() {
        shared.metrics.base_transfers.fetch_add(1, Ordering::Relaxed);
        shared.metrics.base_bytes.fetch_add(bytes, Ordering::Relaxed);
    }
    if hub.take_last_base_chunk().is_some() {
        shared.metrics.base_chunks.fetch_add(1, Ordering::Relaxed);
    }
    if is_op {
        shared.metrics.ops.fetch_add(1, Ordering::Relaxed);
    }
    if is_resync {
        shared.metrics.resyncs.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

fn drain(hub: &Arc<Mutex<Hub>>, peer: &str, ws: &mut tungstenite::WebSocket<TcpStream>, shared: &Arc<Shared>) -> std::io::Result<()> {
    loop {
        let (closed, msg) = {
            let mut hub = hub.lock().map_err(|_| std::io::Error::other("hub lock"))?;
            (hub.is_closed(), hub.poll(peer))
        };
        if closed {
            // `DELETE /v1/sessions/{id}` dropped the room from the server's map; this socket must
            // end before the same store directory backs a new room.
            return Err(std::io::Error::new(std::io::ErrorKind::ConnectionAborted, "the session was closed"));
        }
        let Some(msg) = msg else { return Ok(()) };
        send_msg(ws, &msg, shared)?;
    }
}

fn send_msg(ws: &mut tungstenite::WebSocket<TcpStream>, msg: &ServerMsg, shared: &Arc<Shared>) -> std::io::Result<()> {
    // A refusal is an answer, not an accepted step: `ops_total` counts what arrived, so a room
    // that refuses every structural step would otherwise look like a room that takes them and
    // does nothing. Counting here sees exactly what the clients were told, on every path (an
    // operation, a commit, a base a compaction replaced).
    if matches!(msg, ServerMsg::Rejected(_)) {
        shared.metrics.rejected.fetch_add(1, Ordering::Relaxed);
    }
    let text = encode_server(msg).map_err(std::io::Error::other)?;
    shared.metrics.bytes_out.fetch_add(text.len() as u64, Ordering::Relaxed);
    ws.send(tungstenite::Message::Text(text.into())).map_err(std::io::Error::other)
}

fn send_error(ws: &mut tungstenite::WebSocket<TcpStream>, code: &str, message: &str, retryable: bool) -> std::io::Result<()> {
    let error = ServerError { code: code.into(), message: message.into(), retryable };
    let text = encode_server(&ServerMsg::Error(error)).map_err(std::io::Error::other)?;
    ws.send(tungstenite::Message::Text(text.into())).map_err(std::io::Error::other)?;
    Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, message.to_string()))
}

/// The protocol version this binary speaks, for the health payload and logs.
pub const PROTOCOL: u16 = PROTO_VERSION;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_commit_is_bound_to_the_hub_it_was_authorized_against() {
        use deckcraft_collab::DocSession;
        let config = Config::for_tests(b"secret", "service-token");
        let shared = Arc::new(Shared {
            config,
            hubs: Mutex::new(HashMap::new()),
            names: Mutex::new(HashMap::new()),
            metrics: Metrics::default(),
            store: None,
            cabinet: None,
        });
        let deck = || {
            let engine = deckcraft_engine::Session::with_new();
            engine.doc().expect("document").doc.as_ref().clone()
        };
        let room = |file: &str| {
            let session = DocSession::from_document(file, deck(), SessionConfig::default()).expect("session");
            Arc::new(Mutex::new(Hub::new(session)))
        };
        let hub = room("file-h");
        // A hub that is not the room in the map is refused before anything is built: this is an
        // old socket whose session was deleted and re-created.
        assert!(matches!(build_and_publish_on(&hub, &shared, "file-h", None), Err(CommitFailure::NotFound)));
        shared.hubs.lock().expect("hubs").insert("file-h".into(), hub.clone());
        assert!(build_and_publish_on(&hub, &shared, "file-h", None).is_ok(), "the live room builds and publishes");
        // `DELETE` closes the room before it leaves the map; the socket that outlived it must not
        // build or publish whatever now serves the same file id.
        hub.lock().expect("hub").close();
        assert!(matches!(build_and_publish_on(&hub, &shared, "file-h", None), Err(CommitFailure::NotFound)), "a closed room commits nothing");
        let fresh = room("file-h");
        shared.hubs.lock().expect("hubs").insert("file-h".into(), fresh.clone());
        assert!(
            matches!(build_and_publish_on(&hub, &shared, "file-h", None), Err(CommitFailure::NotFound)),
            "a closed room cannot publish the document of the room that replaced it"
        );
        assert!(matches!(build_and_publish_on(&hub, &shared, "file-h", None), Err(CommitFailure::NotFound)));
        assert!(build_and_publish_on(&fresh, &shared, "file-h", None).is_ok());
    }

    #[test]
    fn a_close_whose_snapshot_fails_keeps_the_room_live() {
        use deckcraft_collab::DocSession;
        use store::Store;

        let root = std::env::temp_dir().join(format!("deckcraft-close-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let config = Config::for_tests(b"secret", "service-token");
        let store = Store::new(&root, 200).expect("store");
        // The room's directory is a regular file: `create_dir_all` in `snapshot` fails, so the
        // close cannot produce the recovery copy it promises.
        let room = root.join(deckcraft_collab::sha256_hex(b"file-c"));
        std::fs::create_dir_all(&root).expect("root");
        std::fs::write(&room, b"not a directory").expect("room path");
        let shared = Arc::new(Shared {
            config,
            hubs: Mutex::new(HashMap::new()),
            names: Mutex::new(HashMap::new()),
            metrics: Metrics::default(),
            store: Some(store),
            cabinet: None,
        });
        let engine = deckcraft_engine::Session::with_new();
        let session =
            DocSession::from_document("file-c", engine.doc().expect("document").doc.as_ref().clone(), SessionConfig::default()).expect("session");
        let hub = Arc::new(Mutex::new(Hub::new(session)));
        shared.hubs.lock().expect("hubs").insert("file-c".into(), hub.clone());
        let request = Request {
            method: "DELETE".into(),
            path: "/v1/sessions/file-c".into(),
            headers: vec![("authorization".into(), "Bearer service-token".into())],
            body: Vec::new(),
        };

        let response = close_session("file-c", &shared, &request).expect("a response");
        assert_eq!(response.status, 500, "a close without its recovery copy is not a close: {}", String::from_utf8_lossy(&response.body));
        assert!(shared.hubs.lock().expect("hubs").contains_key("file-c"), "the hub left the map although its snapshot failed");
        assert!(!hub.lock().expect("hub").is_closed(), "the live room was closed without a snapshot");
        assert!(build_and_publish_on(&hub, &shared, "file-c", None).is_ok(), "the room still serves a commit");

        // Once the store can be written again the same close succeeds and the base is on disk.
        std::fs::remove_file(&room).expect("unsabotage");
        let response = close_session("file-c", &shared, &request).expect("a response");
        assert_eq!(response.status, 200);
        assert!(!shared.hubs.lock().expect("hubs").contains_key("file-c"));
        assert!(hub.lock().expect("hub").is_closed());
        let base =
            std::fs::read_dir(&room).expect("room directory").filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().starts_with("base-"));
        assert!(base, "the close left a base to reopen from");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_token_must_match_the_audience_and_the_role_must_be_known() {
        let config = JwtConfig::Secret(b"secret".to_vec());
        let claims = TokenClaims {
            sub: "alice".into(),
            role: "editor".into(),
            file_id: Some("d".into()),
            app: Some("deckcraft".into()),
            session_id: Some("d".into()),
            aud: Some("office-sync".into()),
            exp: Some(now_secs() + 60),
        };
        let token = jsonwebtoken::encode(&jsonwebtoken::Header::new(Algorithm::HS256), &claims, &jsonwebtoken::EncodingKey::from_secret(b"secret"))
            .expect("token");
        let out = verify_token(&token, &config, "office-sync").expect("valid");
        assert_eq!(out.sub, "alice");
        assert_eq!(role_of(&out.role), Some(Role::Editor));
        assert!(verify_token(&token, &config, "other").is_err());
        assert_eq!(role_of("superuser"), None);
    }

    #[test]
    fn a_base_is_served_by_digest_to_a_token_for_this_file_only() {
        use deckcraft_collab::DocSession;
        let config = Config::for_tests(b"secret", "service-token");
        let shared = Arc::new(Shared {
            config,
            hubs: Mutex::new(HashMap::new()),
            names: Mutex::new(HashMap::new()),
            metrics: Metrics::default(),
            store: None,
            cabinet: None,
        });
        let deck = || {
            let engine = deckcraft_engine::Session::with_new();
            engine.doc().expect("document").doc.as_ref().clone()
        };
        let session = DocSession::from_document("file-b", deck(), SessionConfig::default()).expect("session");
        let hub = Arc::new(Mutex::new(Hub::new(session)));
        let (sha, len) = {
            let mut hub = hub.lock().expect("hub");
            let snapshot = hub.session_mut().take_snapshot().expect("a base");
            (snapshot.sha256.clone(), snapshot.bytes.len())
        };
        shared.hubs.lock().expect("hubs").insert("file-b".into(), hub.clone());

        let token = |actor: &str, role: &str, file: &str| {
            let claims = TokenClaims {
                sub: actor.into(),
                role: role.into(),
                file_id: Some(file.into()),
                app: Some("deckcraft".into()),
                session_id: Some(file.into()),
                aud: Some("office-sync".into()),
                exp: Some(now_secs() + 60),
            };
            jsonwebtoken::encode(&jsonwebtoken::Header::new(Algorithm::HS256), &claims, &jsonwebtoken::EncodingKey::from_secret(b"secret"))
                .expect("token")
        };
        let request = |bearer: Option<&str>, path: &str| Request {
            method: "GET".into(),
            path: path.into(),
            headers: bearer.map(|token| ("authorization".into(), format!("Bearer {token}"))).into_iter().collect(),
            body: Vec::new(),
        };
        let path = format!("/v1/sessions/file-b/base/{sha}");

        // A viewer beneath a compacted journal has to rebuild too, so reading the base is not a
        // write: its token is enough, and the answer is the native format.
        let response = get_base("file-b", &sha, &shared, &request(Some(&token("victor", "viewer", "file-b")), &path));
        assert_eq!(response.status, 200, "{}", String::from_utf8_lossy(&response.body));
        assert_eq!(response.content_type, "application/vnd.deckcraft.native");
        assert_eq!(response.body.len(), len);
        assert_eq!(deckcraft_collab::sha256_hex(&response.body), sha, "the answer is the base the path names");
        assert_eq!(shared.metrics.base_http.load(Ordering::Relaxed), 1, "the transfer is counted");

        assert_eq!(get_base("file-b", &sha, &shared, &request(Some("service-token"), &path)).status, 200, "the cabinet service reads it too");
        assert_eq!(get_base("file-b", &sha, &shared, &request(None, &path)).status, 401, "no token, no document");
        assert_eq!(
            get_base("file-b", &sha, &shared, &request(Some(&token("alice", "editor", "file-other")), &path)).status,
            401,
            "a token for another file does not read this room"
        );
        let other = tombstone();
        assert_eq!(
            get_base("file-b", &other, &shared, &request(Some(&token("alice", "editor", "file-b")), &path)).status,
            404,
            "the path must name the base the session holds"
        );
        assert_eq!(get_base("file-b", "not-a-digest", &shared, &request(Some("service-token"), &path)).status, 404);
        assert_eq!(get_base("file-missing", &sha, &shared, &request(Some("service-token"), &path)).status, 404, "a room that is not loaded");
        assert_eq!(shared.metrics.base_http_bytes.load(Ordering::Relaxed), 2 * len as u64, "only the served bytes are counted");
    }

    /// A digest of something that is not this room's base.
    fn tombstone() -> String {
        deckcraft_collab::sha256_hex(b"a base this room does not hold")
    }

    #[test]
    fn the_http_reader_accepts_a_body_as_large_as_the_widest_media_kind() {
        use std::io::Write as _;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        // 20 MiB: above the old 16 MiB reader cap, inside a video/audio/model's 32 MiB budget.
        let body = vec![0u8; 20 * 1024 * 1024];
        let length = body.len();
        let writer = std::thread::spawn(move || {
            let mut stream = std::net::TcpStream::connect(address).expect("connect");
            write!(stream, "PUT /v1/sessions/x/media/00 HTTP/1.1\r\nContent-Length: {length}\r\n\r\n").expect("head");
            stream.write_all(&body).expect("body");
        });
        let (mut stream, _) = listener.accept().expect("accept");
        let request = read_request(&mut stream).expect("a 20 MiB upload is within the media channel");
        assert_eq!(request.body.len(), length);
        assert_eq!(request.method, "PUT");
        writer.join().expect("writer");
    }
}
