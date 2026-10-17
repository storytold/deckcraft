//! craft-live: the web build as a live editor inside Nextcloud Office.
//!
//! craft-live (a WOPI editor that Nextcloud Office uses instead of Collabora) loads the web build
//! in a frame as `index.html#craft-live=1&ws=…&room=…&token=…` and expects the "embed contract"
//! of its `docs/design.md` §9: say hello to the shell around the frame, seed the room from the
//! file or join it, speak y-websocket (protocol 2, Appendix B: lib0 framing) plus craft-live's
//! control messages (type 100) on the room's WebSocket, upload the file when the room asks for a
//! save, and tell the shell what happens (`craft-live:loaded`, `modified`, `saved`, `peers`,
//! `close`, `error`).
//!
//! [`LiveClient`] does all of that without touching the network or the browser: it reads
//! events (the WebSocket opened, a frame arrived, an HTTP answer came back, the shell posted a
//! message, time passed) and answers [`Effect`]s for the host to carry out (connect, send a
//! frame, make a request, post to the shell, show a message). The browser shell
//! (`apps/deckcraft-web`) runs the effects with `web_sys`; the tests run them against a fake
//! room. The presentation itself is shared through the engine's ordinary co-editing room
//! ([`crate::collab`]), served in memory under the transport id [`TRANSPORT`]: the room's
//! `collab/*` messages map one to one onto the y-websocket frames.

use deckcraft_plugin::push::{Incoming, Pushed};
use serde_json::{Value as Json, json};

use crate::Session;
use crate::collab::{RoomState, User};

/// The transport id of the room a craft-live client keeps (shown as "through craft-live").
pub const TRANSPORT: &str = "craft-live";
/// The embed contract's version (`craft-live=1`).
pub const CONTRACT: u32 = 1;
/// Largest frame accepted from the server: a 16 MiB update plus framing (craft-live's limit).
pub const MAX_FRAME: usize = (16 << 20) + 64;
/// Largest file fetched or uploaded (craft-live's default `max_file_mb`).
pub const MAX_FILE: usize = 100 << 20;
/// Reconnecting waits this long first, doubling up to [`RETRY_MAX_MS`].
pub const RETRY_MIN_MS: u64 = 1_000;
pub const RETRY_MAX_MS: u64 = 30_000;

/// The WebSocket close codes craft-live uses.
pub mod close {
    /// The session expired: open the file again.
    pub const EXPIRED: u16 = 4401;
    /// Refused (the reason also comes as y-websocket's permission-denied message).
    pub const REFUSED: u16 = 4403;
    /// Too slow: reconnect.
    pub const TOO_SLOW: u16 = 4408;
}

// ------------------------------------------------------------------------------------------
// The wire format

/// y-websocket's lib0 framing (protocol 2, Appendix B) with craft-live's control message.
pub mod wire {
    /// A message on the room's WebSocket.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Frame {
        /// `0/0`: sync step 1, a state vector.
        Sync1(Vec<u8>),
        /// `0/1`: sync step 2, an update.
        Sync2(Vec<u8>),
        /// `0/2`: an update.
        Update(Vec<u8>),
        /// `1`: a y-protocols awareness update.
        Awareness(Vec<u8>),
        /// `2/0`: permission denied, with the reason.
        Denied(String),
        /// `3`: asks for everyone's awareness.
        QueryAwareness,
        /// `100`: a craft-live control message (JSON text).
        Control(String),
    }

    /// The control message's type number.
    pub const CONTROL: u64 = 100;

    /// Appends a lib0 varuint.
    pub fn write_var(out: &mut Vec<u8>, mut n: u64) {
        loop {
            let byte = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }

    /// Appends lib0 varbytes: the length, then the bytes.
    pub fn write_bytes(out: &mut Vec<u8>, data: &[u8]) {
        write_var(out, data.len() as u64);
        out.extend_from_slice(data);
    }

    /// Reads a frame. Everything is input from the network: nothing here panics, and lengths
    /// that run past the end are refused.
    struct Reader<'a> {
        data: &'a [u8],
        pos: usize,
    }

    impl<'a> Reader<'a> {
        fn var(&mut self) -> Option<u64> {
            let mut n: u64 = 0;
            let mut shift = 0u32;
            loop {
                let b = *self.data.get(self.pos)?;
                self.pos += 1;
                if shift >= 64 {
                    return None;
                }
                let part = u64::from(b & 0x7f);
                if shift == 63 && part > 1 {
                    return None;
                }
                n |= part << shift;
                if b & 0x80 == 0 {
                    return Some(n);
                }
                shift += 7;
            }
        }
        fn bytes(&mut self) -> Option<&'a [u8]> {
            let len = usize::try_from(self.var()?).ok()?;
            let end = self.pos.checked_add(len)?;
            let out = self.data.get(self.pos..end)?;
            self.pos = end;
            Some(out)
        }
        fn string(&mut self) -> Option<&'a str> {
            std::str::from_utf8(self.bytes()?).ok()
        }
    }

    impl Frame {
        pub fn encode(&self) -> Vec<u8> {
            let mut out = Vec::new();
            match self {
                Frame::Sync1(d) => {
                    out.extend_from_slice(&[0, 0]);
                    write_bytes(&mut out, d);
                }
                Frame::Sync2(d) => {
                    out.extend_from_slice(&[0, 1]);
                    write_bytes(&mut out, d);
                }
                Frame::Update(d) => {
                    out.extend_from_slice(&[0, 2]);
                    write_bytes(&mut out, d);
                }
                Frame::Awareness(d) => {
                    out.push(1);
                    write_bytes(&mut out, d);
                }
                Frame::Denied(reason) => {
                    out.extend_from_slice(&[2, 0]);
                    write_bytes(&mut out, reason.as_bytes());
                }
                Frame::QueryAwareness => out.push(3),
                Frame::Control(text) => {
                    write_var(&mut out, CONTROL);
                    write_bytes(&mut out, text.as_bytes());
                }
            }
            out
        }

        /// `None` for anything malformed or unknown (unknown types are ignored).
        pub fn decode(data: &[u8]) -> Option<Frame> {
            let mut r = Reader { data, pos: 0 };
            Some(match r.var()? {
                0 => {
                    let step = r.var()?;
                    let payload = r.bytes()?.to_vec();
                    match step {
                        0 => Frame::Sync1(payload),
                        1 => Frame::Sync2(payload),
                        2 => Frame::Update(payload),
                        _ => return None,
                    }
                }
                1 => Frame::Awareness(r.bytes()?.to_vec()),
                2 => {
                    if r.var()? != 0 {
                        return None;
                    }
                    Frame::Denied(r.string()?.to_string())
                }
                3 => Frame::QueryAwareness,
                CONTROL => Frame::Control(r.string()?.to_string()),
                _ => return None,
            })
        }

        /// A control message.
        pub fn control(v: &serde_json::Value) -> Frame {
            Frame::Control(v.to_string())
        }
    }
}

use wire::Frame;

// ------------------------------------------------------------------------------------------
// Configuration

/// The embed configuration (design §9.1), from the page's fragment or the shell's
/// `craft-live:config`.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// The room's WebSocket URL (connect to `<ws>?token=<token>`).
    pub ws: String,
    pub room: String,
    /// The session token (secret): the WebSocket's `token` and the API's bearer token.
    pub token: String,
    pub user: Option<User>,
    /// `view`: read-only.
    pub view: bool,
    /// This client seeds the room from the file.
    pub seed: bool,
    /// Where the file's bytes are (`GET` with the bearer token).
    pub file: String,
    /// The file's name (`Talk.pptx`).
    pub name: String,
    /// Its extension: the format to save in.
    pub format: String,
    /// The API base (`https://live.example.com/api/v1`).
    pub api: String,
    pub lang: String,
}

/// Decodes `%XX` and `+` (form encoding). Bad escapes stay as they are.
pub fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        match c {
            b'%' => match b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                Some(v) => {
                    out.push(v);
                    i += 3;
                }
                None => {
                    out.push(c);
                    i += 1;
                }
            },
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encodes everything but `A-Z a-z 0-9 - . _ ~`.
pub fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The person in the configuration: `{id, name, color}` (a colour that isn't `#RRGGBB` becomes
/// grey; names are cut to 128 characters).
fn person(v: &Json) -> Option<User> {
    let s = |k: &str| v.get(k).and_then(Json::as_str).map(|t| t.trim().chars().take(128).collect::<String>()).filter(|t| !t.is_empty());
    let color = s("color").filter(|c| crate::collab::valid_color(c)).unwrap_or_else(|| "#808080".into());
    Some(User { id: s("id")?, name: s("name")?, color })
}

impl Config {
    /// The configuration in a page's fragment (`#craft-live=1&…`, with or without the `#`).
    /// `None` when the fragment isn't craft-live's; an error when it is but can't be used.
    pub fn from_fragment(fragment: &str) -> Option<Result<Config, String>> {
        let f = fragment.strip_prefix('#').unwrap_or(fragment);
        let mut map = serde_json::Map::new();
        for part in f.split('&').take(64) {
            let (k, v) = part.split_once('=').unwrap_or((part, ""));
            map.insert(pct_decode(k), Json::String(pct_decode(v)));
        }
        let version = map.get("craft-live").and_then(Json::as_str)?.to_string();
        if version != CONTRACT.to_string() {
            return Some(Err(format!("this DeckCraft speaks craft-live {CONTRACT}, not {version}")));
        }
        if let Some(u) = map.get("user").and_then(Json::as_str).and_then(|t| serde_json::from_str::<Json>(t).ok()) {
            map.insert("user".into(), u);
        }
        if let Some(s) = map.get("seed").and_then(Json::as_str) {
            let seed = s == "1" || s == "true";
            map.insert("seed".into(), Json::Bool(seed));
        }
        Some(Config::from_json(&Json::Object(map)))
    }

    /// The configuration in `craft-live:config` (`seed` a boolean, `user` an object).
    pub fn from_json(v: &Json) -> Result<Config, String> {
        let s = |k: &str| v.get(k).and_then(Json::as_str).map(|t| t.chars().take(4096).collect::<String>()).unwrap_or_default();
        let c = Config {
            ws: s("ws"),
            room: s("room"),
            token: s("token"),
            user: v.get("user").and_then(person),
            view: s("role") == "view",
            seed: v.get("seed").and_then(Json::as_bool).unwrap_or(false),
            file: s("file"),
            name: s("name"),
            format: s("format").to_ascii_lowercase(),
            api: s("api").trim_end_matches('/').to_string(),
            lang: s("lang"),
        };
        if !(c.ws.starts_with("ws://") || c.ws.starts_with("wss://")) {
            return Err("the room's address (ws) is missing".into());
        }
        if c.room.is_empty() || c.token.is_empty() {
            return Err("the room or the session token is missing".into());
        }
        if c.seed && c.file.is_empty() {
            return Err("this session must load the file, but its address is missing".into());
        }
        Ok(c)
    }

    /// The WebSocket URL with the token.
    pub fn ws_url(&self) -> String {
        let sep = if self.ws.contains('?') { '&' } else { '?' };
        format!("{}{sep}token={}", self.ws, pct_encode(&self.token))
    }

    /// The file's name without its extension (the presentation's title).
    pub fn title(&self) -> String {
        std::path::Path::new(&self.name)
            .file_stem()
            .map(|n| n.to_string_lossy().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "Presentation".into())
    }

    fn bearer(&self) -> (String, String) {
        ("Authorization".into(), format!("Bearer {}", self.token))
    }
}

/// The format to write for a save that asks for `format` and a file named `name`: DeckCraft's
/// own kinds of PresentationML package as asked; anything else (`odp`) as `.pptx` under a
/// `.pptx` name, which craft-live then saves as a copy beside the file, never over it.
pub fn save_target(format: &str, name: &str) -> (&'static str, String) {
    let asked = format.trim().to_ascii_lowercase();
    if let Some(kind) = deckcraft_pptx::PptxKind::from_ext(&asked) {
        return (kind.ext(), name.to_string());
    }
    let stem = std::path::Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Presentation".into());
    ("pptx", format!("{stem}.pptx"))
}

// ------------------------------------------------------------------------------------------
// The client

/// What the host must do.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Open the room's WebSocket at this URL (binary frames). Report [`LiveClient::on_open`],
    /// [`LiveClient::on_frame`] and [`LiveClient::on_close`].
    Connect(String),
    /// Send a binary frame on the open WebSocket.
    Send(Vec<u8>),
    /// Close the WebSocket (normal closure); no `on_close` is expected.
    Disconnect,
    /// An HTTP request; its answer goes to [`LiveClient::on_http`].
    Http(HttpRequest),
    /// A message to the craft-live shell (`window.parent.postMessage(…, location.origin)`).
    Post(Json),
    /// A short message for the status bar.
    Status(String),
    /// A message the person must see.
    Alert(String),
}

/// An HTTP request for the host to make.
#[derive(Clone, Debug, PartialEq)]
pub struct HttpRequest {
    pub id: u64,
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

/// What a request answered. `status` 0: it failed before an answer (`body` holds the reason).
#[derive(Clone, Debug, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn failed(why: &str) -> HttpResponse {
        HttpResponse { status: 0, body: why.as_bytes().to_vec() }
    }
    fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
    fn json(&self) -> Json {
        serde_json::from_slice(&self.body).unwrap_or(Json::Null)
    }
    /// The server's error message, or the status.
    fn reason(&self) -> String {
        if self.status == 0 {
            return String::from_utf8_lossy(&self.body).chars().take(300).collect();
        }
        match self.json().get("error").and_then(Json::as_str) {
            Some(e) => e.chars().take(300).collect(),
            None => format!("HTTP {}", self.status),
        }
    }
}

/// Where the client is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Waiting for `craft-live:config` (no fragment).
    Configuring,
    /// Loading the file to seed the room.
    Loading,
    /// Asking the server again for the seed lease (after `reseed`).
    Preparing,
    /// The WebSocket is opening.
    Connecting,
    /// Connected.
    Online,
    /// Disconnected; reconnecting at the time given.
    Offline { retry_at: u64 },
    /// The session expired (close 4401): the person must open the file again.
    Expired,
    /// The server refused the session (close 4403).
    Refused(String),
    /// The person closed the presentation, or the shell did.
    Closed,
    /// The presentation couldn't be opened.
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Purpose {
    File,
    Prepare,
    Upload { save: u64 },
}

/// The craft-live client of one embedded presentation.
pub struct LiveClient {
    pub config: Option<Config>,
    pub phase: Phase,
    /// The WebSocket is open.
    open: bool,
    /// The server's `welcome` arrived on this connection, and the room may get our frames.
    welcomed: bool,
    /// Frames held until `welcome` (the room learns who we are first).
    held: Vec<Vec<u8>>,
    /// Our room is the seeding kind (`collab.share`): a lost lease must not send its content.
    seeding: bool,
    retry_ms: u64,
    next_http: u64,
    pending: Vec<(u64, Purpose)>,
    /// Messages for the engine's room, for the next [`LiveClient::pump`].
    inbox: Vec<Pushed>,
    /// `craft-live:loaded` was posted.
    loaded: bool,
    /// The person asked to save (Save, or Nextcloud's `Action_Save`): `craft-live:saved` follows.
    save_asked: bool,
    /// The room's unsaved state as the server last said.
    pub modified: bool,
    /// The others, as the server's last `peers` listed them.
    pub peers: Vec<Json>,
    /// The last `welcome`.
    pub welcome: Option<Json>,
    /// The reason of a permission-denied message.
    denied: Option<String>,
    /// The engine's `collab/leave` is ours (reseeding, closing): don't treat it as Close.
    quiet_leave: bool,
    /// Saves asked and not answered: (save id).
    saves: Vec<u64>,
}

impl Default for LiveClient {
    fn default() -> Self {
        LiveClient::new()
    }
}

fn notification(method: &str, params: Json) -> Pushed {
    Pushed { plugin: TRANSPORT.to_string(), incoming: Incoming::Message(json!({"jsonrpc": "2.0", "method": method, "params": params})) }
}

impl LiveClient {
    pub fn new() -> LiveClient {
        LiveClient {
            config: None,
            phase: Phase::Configuring,
            open: false,
            welcomed: false,
            held: Vec::new(),
            seeding: false,
            retry_ms: RETRY_MIN_MS,
            next_http: 1,
            pending: Vec::new(),
            inbox: Vec::new(),
            loaded: false,
            save_asked: false,
            modified: false,
            peers: Vec::new(),
            welcome: None,
            denied: None,
            quiet_leave: false,
            saves: Vec::new(),
        }
    }

    /// The first message to the shell, as soon as the app runs.
    pub fn hello() -> Effect {
        Effect::Post(json!({"type": "craft-live:hello"}))
    }

    /// The room's id, once configured.
    pub fn room(&self) -> Option<&str> {
        self.config.as_ref().map(|c| c.room.as_str())
    }

    fn room_of<'a>(&self, s: &'a Session) -> Option<&'a crate::collab::Room> {
        let id = self.room()?;
        s.collab.rooms.iter().find(|r| r.room == id)
    }

    fn http(&mut self, purpose: Purpose, method: &'static str, url: String, headers: Vec<(String, String)>, body: Option<Vec<u8>>) -> Effect {
        let id = self.next_http;
        self.next_http = self.next_http.wrapping_add(1).max(1);
        self.pending.push((id, purpose));
        Effect::Http(HttpRequest { id, method, url, headers, body })
    }

    fn fail(&mut self, why: String) -> Vec<Effect> {
        self.phase = Phase::Failed(why.clone());
        let mut out = vec![Effect::Alert(why.clone())];
        if !self.loaded {
            out.push(Effect::Post(json!({"type": "craft-live:error", "message": why})));
        }
        out
    }

    /// Starts with a configuration (from the fragment, or the shell's `craft-live:config`).
    /// Does nothing when already started.
    pub fn start(&mut self, s: &mut Session, config: Config, now: u64) -> Vec<Effect> {
        if self.config.is_some() {
            return Vec::new();
        }
        let seed = config.seed && !config.view;
        self.config = Some(config);
        s.collab_serve_in_memory(TRANSPORT);
        if seed { self.load_file() } else { self.join(s, now) }
    }

    fn load_file(&mut self) -> Vec<Effect> {
        let Some(c) = self.config.clone() else { return Vec::new() };
        self.phase = Phase::Loading;
        let req = self.http(Purpose::File, "GET", c.file.clone(), vec![c.bearer()], None);
        vec![Effect::Status(format!("Opening {}…", c.name)), req]
    }

    /// Joins the room (`collab.join` mode `join`): read-only, "Joining…", until the room's
    /// document arrives.
    fn join(&mut self, s: &mut Session, now: u64) -> Vec<Effect> {
        let Some(c) = self.config.clone() else { return Vec::new() };
        self.seeding = false;
        let mut p = json!({"plugin": TRANSPORT, "room": c.room, "mode": "join", "role": if c.view { "view" } else { "edit" }});
        if let Some(u) = &c.user {
            p["user"] = json!(u);
        }
        if let Err(e) = s.execute("collab.join", &p) {
            return self.fail(format!("The presentation couldn't be opened: {e}"));
        }
        self.only_the_room(s);
        self.connect(now)
    }

    /// Seeds the room from the file's bytes (`collab.share` semantics).
    fn seed(&mut self, s: &mut Session, bytes: &[u8], now: u64) -> Vec<Effect> {
        let Some(c) = self.config.clone() else { return Vec::new() };
        let opened = if bytes.is_empty() {
            // A 0-byte file is a new, empty presentation.
            s.execute("file.new", &json!({})).map(|_| ())
        } else {
            s.execute("file.openBytes", &json!({"name": c.name, "data": crate::cmd::base64_encode(bytes)})).map(|_| ())
        };
        if let Err(e) = opened {
            return self.fail(format!("DeckCraft can't open {}: {e}", c.name));
        }
        if let Ok(d) = s.doc_mut() {
            d.title = c.title();
        }
        let mut p = json!({"plugin": TRANSPORT, "room": c.room, "mode": "share", "role": "edit", "title": c.title()});
        if let Some(u) = &c.user {
            p["user"] = json!(u);
        }
        if let Err(e) = s.execute("collab.join", &p) {
            return self.fail(format!("The presentation couldn't be shared: {e}"));
        }
        self.seeding = true;
        self.only_the_room(s);
        self.connect(now)
    }

    /// Closes every presentation but the room's (the placeholder shown while loading, an empty
    /// one left by a reseed), and makes the room's the active one.
    fn only_the_room(&self, s: &mut Session) {
        let Some(uid) = self.room_of(s).map(|r| r.doc) else { return };
        while let Some(i) = s.documents().iter().position(|d| d.uid != uid) {
            s.close_document(i);
        }
        if let Some(i) = s.documents().iter().position(|d| d.uid == uid) {
            s.set_active(i);
        }
    }

    fn connect(&mut self, _now: u64) -> Vec<Effect> {
        let Some(c) = &self.config else { return Vec::new() };
        self.phase = Phase::Connecting;
        self.open = false;
        self.welcomed = false;
        self.held.clear();
        self.denied = None;
        vec![Effect::Connect(c.ws_url())]
    }

    /// The WebSocket opened: `hello`, then (after `welcome`) sync step 1.
    pub fn on_open(&mut self, s: &mut Session, _now: u64) -> Vec<Effect> {
        let Some(r) = self.room_of(s) else { return Vec::new() };
        let client_id = r.client_id();
        let room = r.room.clone();
        self.open = true;
        self.welcomed = false;
        self.phase = Phase::Online;
        // The engine's room asks with sync step 1 when it hears it's online again.
        self.inbox.push(notification("collab/status", json!({"room": room, "state": "online"})));
        let hello =
            json!({"type": "hello", "clientId": client_id, "can": ["upload"], "client": format!("deckcraft-web/{}", env!("CARGO_PKG_VERSION"))});
        vec![Effect::Send(Frame::control(&hello).encode())]
    }

    /// The WebSocket closed (or failed to open).
    pub fn on_close(&mut self, _s: &mut Session, code: u16, reason: &str, now: u64) -> Vec<Effect> {
        let was_open = self.open;
        self.open = false;
        self.welcomed = false;
        self.held.clear();
        if matches!(self.phase, Phase::Closed | Phase::Failed(_) | Phase::Expired | Phase::Refused(_) | Phase::Loading | Phase::Preparing) {
            return Vec::new();
        }
        let room = self.room().unwrap_or("").to_string();
        let offline = |message: &str| notification("collab/status", json!({"room": room, "state": "offline", "message": message}));
        match code {
            close::EXPIRED => {
                self.phase = Phase::Expired;
                self.inbox.push(offline("The editing session ended"));
                let why = "Your editing session has ended. Close the presentation and open it again from Nextcloud.".to_string();
                let mut out = vec![Effect::Alert(why.clone())];
                if !self.loaded {
                    out.push(Effect::Post(json!({"type": "craft-live:error", "message": why})));
                }
                out
            }
            close::REFUSED => {
                let why = self
                    .denied
                    .clone()
                    .filter(|d| !d.is_empty())
                    .unwrap_or_else(|| if reason.is_empty() { "craft-live refused this session".into() } else { reason.to_string() });
                self.phase = Phase::Refused(why.clone());
                self.inbox.push(offline(&why));
                let mut out = vec![Effect::Alert(format!("The presentation can't be edited live: {why}"))];
                if !self.loaded {
                    out.push(Effect::Post(json!({"type": "craft-live:error", "message": why})));
                }
                out
            }
            _ => {
                let wait = self.retry_ms;
                self.retry_ms = (self.retry_ms.saturating_mul(2)).min(RETRY_MAX_MS);
                self.phase = Phase::Offline { retry_at: now.saturating_add(wait) };
                self.inbox.push(offline("Offline: reconnecting…"));
                if was_open { vec![Effect::Status("Offline: reconnecting to craft-live…".into())] } else { Vec::new() }
            }
        }
    }

    /// A binary frame arrived.
    pub fn on_frame(&mut self, s: &mut Session, data: &[u8], now: u64) -> Vec<Effect> {
        if data.len() > MAX_FRAME || !self.open {
            return Vec::new();
        }
        let Some(frame) = Frame::decode(data) else { return Vec::new() };
        let room = self.room().unwrap_or("").to_string();
        let b64 = deckcraft_collab::b64_encode;
        match frame {
            Frame::Sync1(sv) => self.inbox.push(notification("collab/sync1", json!({"room": room, "stateVector": b64(&sv)}))),
            Frame::Sync2(u) => self.inbox.push(notification("collab/sync2", json!({"room": room, "update": b64(&u)}))),
            Frame::Update(u) => self.inbox.push(notification("collab/update", json!({"room": room, "update": b64(&u)}))),
            Frame::Awareness(u) => self.inbox.push(notification("collab/awareness", json!({"room": room, "update": b64(&u)}))),
            Frame::Denied(why) => self.denied = Some(why.chars().take(300).collect()),
            Frame::QueryAwareness => {}
            Frame::Control(text) => {
                let Ok(v) = serde_json::from_str::<Json>(&text) else { return Vec::new() };
                return self.on_control(s, &v, now);
            }
        }
        Vec::new()
    }

    fn on_control(&mut self, s: &mut Session, v: &Json, now: u64) -> Vec<Effect> {
        let room = self.room().unwrap_or("").to_string();
        match v.get("type").and_then(Json::as_str).unwrap_or("") {
            "welcome" => {
                self.welcome = Some(v.clone());
                self.modified = v.get("modified").and_then(Json::as_bool).unwrap_or(false);
                let seeded = v.get("seeded").and_then(Json::as_bool).unwrap_or(false);
                let lease = v.get("seed").and_then(Json::as_bool).unwrap_or(false);
                if self.seeding && !seeded && !lease {
                    // We loaded the file, but the lease went to someone else: two seeds would
                    // duplicate the content. Ask again.
                    return self.restart(s, Some("Someone else is loading the presentation…".into()));
                }
                self.welcomed = true;
                self.retry_ms = RETRY_MIN_MS;
                let mut out: Vec<Effect> = self.held.drain(..).map(Effect::Send).collect();
                out.push(Effect::Post(json!({"type": "craft-live:modified", "modified": self.modified})));
                out
            }
            "peers" => {
                self.peers = v.get("peers").and_then(Json::as_array).map(|a| a.iter().take(1000).cloned().collect()).unwrap_or_default();
                let listed: Vec<Json> = self.peers.iter().filter_map(|p| p.get("user").cloned()).map(|u| json!({"user": u})).collect();
                self.inbox.push(notification("collab/status", json!({"room": room, "state": "online", "peers": listed})));
                vec![self.peers_message(s)]
            }
            "save" => self.on_save(s, v),
            "saved" => {
                self.modified = v.get("modified").and_then(Json::as_bool).unwrap_or(false);
                self.sync_dirty(s);
                let mut out = vec![Effect::Post(json!({"type": "craft-live:modified", "modified": self.modified}))];
                if std::mem::take(&mut self.save_asked) {
                    out.push(Effect::Post(json!({"type": "craft-live:saved", "success": true})));
                    out.push(Effect::Status("Saved to Nextcloud".into()));
                }
                out
            }
            "saveFailed" => {
                let why: String = v.get("message").and_then(Json::as_str).unwrap_or("the save failed").chars().take(300).collect();
                let mut out = vec![Effect::Status(format!("Not saved to Nextcloud: {why}"))];
                if std::mem::take(&mut self.save_asked) {
                    out.push(Effect::Post(json!({"type": "craft-live:saved", "success": false})));
                }
                out
            }
            "modified" | "seeded" => {
                self.modified = v.get("modified").and_then(Json::as_bool).unwrap_or(false);
                self.sync_dirty(s);
                vec![Effect::Post(json!({"type": "craft-live:modified", "modified": self.modified}))]
            }
            "notice" => {
                let text: String = v.get("text").and_then(Json::as_str).unwrap_or("").chars().take(500).collect();
                if text.is_empty() {
                    return Vec::new();
                }
                match v.get("level").and_then(Json::as_str) {
                    Some("warning" | "error") => vec![Effect::Alert(text)],
                    _ => vec![Effect::Status(text)],
                }
            }
            "reseed" => {
                let why = v.get("message").and_then(Json::as_str).map(|m| m.chars().take(300).collect());
                self.restart(s, why)
            }
            "expired" => {
                self.phase = Phase::Expired;
                vec![Effect::Alert("Your editing session has ended. Close the presentation and open it again from Nextcloud.".into())]
            }
            _ => {
                let _ = now;
                Vec::new()
            }
        }
    }

    /// The presentation shows "Edited" while the room has unsaved changes: once the room says
    /// it has none (saved into Nextcloud), it's clean here too.
    fn sync_dirty(&self, s: &mut Session) {
        if self.modified {
            return;
        }
        let Some(uid) = self.room_of(s).map(|r| r.doc) else { return };
        if let Some(d) = s.docs.iter_mut().find(|d| d.uid == uid) {
            d.saved_doc = d.doc.clone();
            d.revision += 1;
        }
    }

    /// Leaves the room here (not as a Close), disconnects and asks for the seed lease again
    /// (`POST <api>/prepare`): with it, the file is loaded and seeded; without it, joined.
    fn restart(&mut self, s: &mut Session, why: Option<String>) -> Vec<Effect> {
        let Some(c) = self.config.clone() else { return Vec::new() };
        let mut out = Vec::new();
        if self.open {
            out.push(Effect::Disconnect);
        }
        self.open = false;
        self.welcomed = false;
        self.held.clear();
        if let Some(room) = self.room_of(s).map(|r| r.room.clone()) {
            self.quiet_leave = true;
            let _ = s.execute("collab.leave", &json!({"room": room}));
            self.drain_outgoing(s);
            self.quiet_leave = false;
        }
        self.phase = Phase::Preparing;
        if let Some(w) = why {
            out.push(Effect::Status(w));
        }
        out.push(self.http(Purpose::Prepare, "POST", format!("{}/prepare", c.api), vec![c.bearer()], None));
        out
    }

    fn on_save(&mut self, s: &mut Session, v: &Json) -> Vec<Effect> {
        let Some(c) = self.config.clone() else { return Vec::new() };
        let id = v.get("id").and_then(Json::as_u64).unwrap_or(0);
        let refuse = |why: &str| vec![Effect::Send(Frame::control(&json!({"type": "saveResult", "id": id, "ok": false, "error": why})).encode())];
        if v.get("mode").and_then(Json::as_str) != Some("upload") {
            return refuse("the web app saves by upload only");
        }
        let url = v.get("url").and_then(Json::as_str).unwrap_or("").to_string();
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return refuse("no upload address");
        }
        let Some(room) = self.room_of(s) else { return refuse("not in the room") };
        if room.joining || room.refused.is_some() {
            return refuse("the presentation hasn't arrived yet");
        }
        let uid = room.doc;
        let Some(doc) = s.documents().iter().find(|d| d.uid == uid).map(|d| d.doc.clone()) else { return refuse("the presentation was closed") };
        let format = v.get("format").and_then(Json::as_str).unwrap_or(&c.format).to_string();
        let name = v.get("name").and_then(Json::as_str).filter(|n| !n.is_empty()).unwrap_or(&c.name).to_string();
        let (fmt, name) = save_target(&format, &name);
        let bytes = match crate::cmd::file::save_bytes(&doc, fmt) {
            Ok(b) if b.len() <= MAX_FILE => b,
            Ok(b) => return refuse(&format!("the presentation is {} MB, more than craft-live takes", b.len() >> 20)),
            Err(e) => return refuse(&e.to_string()),
        };
        self.saves.push(id);
        let headers = vec![c.bearer(), ("X-Craft-File-Name".into(), pct_encode(&name)), ("Content-Type".into(), "application/octet-stream".into())];
        vec![self.http(Purpose::Upload { save: id }, "PUT", url, headers, Some(bytes))]
    }

    /// An HTTP request answered.
    pub fn on_http(&mut self, s: &mut Session, id: u64, r: HttpResponse, now: u64) -> Vec<Effect> {
        let Some(i) = self.pending.iter().position(|(p, _)| *p == id) else { return Vec::new() };
        let (_, purpose) = self.pending.remove(i);
        match purpose {
            Purpose::File => {
                if self.phase != Phase::Loading {
                    return Vec::new();
                }
                if !r.ok() {
                    return self.fail(format!("The presentation couldn't be loaded from Nextcloud: {}", r.reason()));
                }
                if r.body.len() > MAX_FILE {
                    return self.fail("The presentation is larger than craft-live takes.".into());
                }
                self.seed(s, &r.body, now)
            }
            Purpose::Prepare => {
                if self.phase != Phase::Preparing {
                    return Vec::new();
                }
                if !r.ok() {
                    if r.status == 401 || r.status == 403 {
                        self.phase = Phase::Expired;
                        return vec![Effect::Alert(
                            "Your editing session has ended. Close the presentation and open it again from Nextcloud.".into(),
                        )];
                    }
                    // Try again in a while, as a reconnect.
                    let wait = self.retry_ms;
                    self.retry_ms = (self.retry_ms.saturating_mul(2)).min(RETRY_MAX_MS);
                    self.phase = Phase::Offline { retry_at: now.saturating_add(wait) };
                    return Vec::new();
                }
                let v = r.json();
                if v.get("seed").and_then(Json::as_bool) == Some(true) { self.load_file() } else { self.join(s, now) }
            }
            Purpose::Upload { save } => {
                self.saves.retain(|x| *x != save);
                if r.ok() {
                    return Vec::new();
                }
                // The server answers the room with `saveFailed`; tell it this client's side too.
                let msg = Frame::control(&json!({"type": "saveResult", "id": save, "ok": false, "error": r.reason()})).encode();
                if self.open { vec![Effect::Send(msg)] } else { Vec::new() }
            }
        }
    }

    /// A message from the shell (`craft-live:config`, `save`, `close`, `getPeers`, `host`).
    pub fn on_shell(&mut self, s: &mut Session, msg: &Json, now: u64) -> Vec<Effect> {
        match msg.get("type").and_then(Json::as_str).unwrap_or("") {
            "craft-live:config" => {
                if self.config.is_some() {
                    return Vec::new();
                }
                match Config::from_json(msg.get("config").unwrap_or(&Json::Null)) {
                    Ok(c) => self.start(s, c, now),
                    Err(e) => self.fail(format!("craft-live's configuration can't be used: {e}")),
                }
            }
            "craft-live:save" => {
                self.save_asked = true;
                vec![Effect::Status("Saving to Nextcloud…".into())]
            }
            "craft-live:close" => self.shut(s, false),
            "craft-live:getPeers" => vec![self.peers_message(s)],
            _ => Vec::new(),
        }
    }

    /// The person pressed Save: the room saves now.
    pub fn save_now(&mut self, force: bool) -> Vec<Effect> {
        let editable = self.config.as_ref().is_some_and(|c| !c.view);
        if !editable {
            return vec![Effect::Status("You can view this presentation; saving is up to its editors".into())];
        }
        if !self.open || !self.welcomed {
            return vec![Effect::Status("Not connected: the presentation is saved when craft-live is back".into())];
        }
        self.save_asked = true;
        let mut v = json!({"type": "saveNow"});
        if force {
            v["force"] = json!(true);
        }
        vec![Effect::Send(Frame::control(&v).encode()), Effect::Status("Saving to Nextcloud…".into())]
    }

    /// The person closes the presentation (the Close button, File › Close, the session's
    /// Leave): leave the room, say `bye`, and ask the shell to close the editor.
    pub fn close(&mut self, s: &mut Session) -> Vec<Effect> {
        self.shut(s, true)
    }

    fn shut(&mut self, s: &mut Session, tell_shell: bool) -> Vec<Effect> {
        if self.phase == Phase::Closed {
            return Vec::new();
        }
        let mut out = Vec::new();
        if let Some(room) = self.room_of(s).map(|r| r.room.clone()) {
            self.quiet_leave = true;
            let _ = s.execute("collab.leave", &json!({"room": room}));
            out.extend(self.drain_outgoing(s));
            self.quiet_leave = false;
        }
        if self.open {
            out.push(Effect::Send(Frame::control(&json!({"type": "bye"})).encode()));
            out.push(Effect::Disconnect);
        }
        self.open = false;
        self.phase = Phase::Closed;
        if tell_shell {
            out.push(Effect::Post(json!({"type": "craft-live:close"})));
        }
        out
    }

    /// `craft-live:peers`: the others and this person (`self: true`).
    fn peers_message(&self, s: &Session) -> Effect {
        let mut peers: Vec<Json> = self.peers.clone();
        if let (Some(c), Some(r)) = (&self.config, self.room_of(s)) {
            peers.insert(0, json!({"clientId": r.client_id(), "user": r.user, "role": if c.view { "view" } else { "edit" }, "self": true}));
        }
        Effect::Post(json!({"type": "craft-live:peers", "peers": peers}))
    }

    /// Runs between frames: hands what arrived to the engine's room, sends what the room says,
    /// reconnects when it's time, and posts `craft-live:loaded` once the presentation is shown.
    pub fn pump(&mut self, s: &mut Session, now: u64) -> Vec<Effect> {
        let mut out = Vec::new();
        if let Phase::Offline { retry_at } = self.phase
            && now >= retry_at
        {
            if self.room_of(s).is_some() {
                out.extend(self.connect(now));
            } else if let Some(c) = self.config.clone() {
                // A failed prepare: ask again.
                self.phase = Phase::Preparing;
                out.push(self.http(Purpose::Prepare, "POST", format!("{}/prepare", c.api), vec![c.bearer()], None));
            }
        }
        let pushed = std::mem::take(&mut self.inbox);
        if !pushed.is_empty() || self.room_of(s).is_some() {
            s.collab_poll_pushed(now, pushed);
        }
        out.extend(self.drain_outgoing(s));
        if !self.loaded
            && let Some(r) = self.room_of(s)
            && !r.joining
            && r.refused.is_none()
        {
            self.loaded = true;
            out.push(Effect::Post(json!({"type": "craft-live:loaded"})));
            out.push(self.peers_message(s));
        }
        out
    }

    /// The room's outgoing `collab/*` messages as frames (held until `welcome`; dropped while
    /// offline, since the next connection's sync brings both sides up to date).
    fn drain_outgoing(&mut self, s: &mut Session) -> Vec<Effect> {
        let room = self.room().unwrap_or("").to_string();
        let mut out = Vec::new();
        for m in s.collab_take_outgoing() {
            let method = m.get("method").and_then(Json::as_str).unwrap_or("");
            let p = m.get("params").cloned().unwrap_or(Json::Null);
            if p.get("room").and_then(Json::as_str).is_some_and(|r| r != room) {
                continue;
            }
            let bytes = |k: &str| p.get(k).and_then(Json::as_str).and_then(|t| deckcraft_collab::b64_decode(t).ok());
            let frame = match method {
                "collab/sync1" => bytes("stateVector").map(Frame::Sync1),
                "collab/sync2" => bytes("update").map(Frame::Sync2),
                "collab/update" => bytes("update").map(Frame::Update),
                "collab/awareness" => bytes("update").map(Frame::Awareness),
                "collab/leave" => {
                    if !self.quiet_leave && self.phase != Phase::Closed {
                        // The person left the session (its Leave, or File › Close): in Nextcloud
                        // that closes the editor.
                        if self.open {
                            out.push(Effect::Send(Frame::control(&json!({"type": "bye"})).encode()));
                            out.push(Effect::Disconnect);
                        }
                        self.open = false;
                        self.phase = Phase::Closed;
                        out.push(Effect::Post(json!({"type": "craft-live:close"})));
                    }
                    None
                }
                _ => None,
            };
            let Some(frame) = frame else { continue };
            if !self.open {
                continue;
            }
            let data = frame.encode();
            if self.welcomed {
                out.push(Effect::Send(data));
            } else if self.held.len() < 10_000 {
                self.held.push(data);
            }
        }
        out
    }

    /// Whether the room's state is a live connection (for the interface).
    pub fn online(&self) -> bool {
        self.open && self.welcomed
    }

    /// The room's state for the engine's status: whether it's offline.
    pub fn room_state(&self, s: &Session) -> Option<RoomState> {
        self.room_of(s).map(|r| r.state)
    }

    /// Requests in flight (tests, diagnostics).
    pub fn pending_requests(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod tests;
