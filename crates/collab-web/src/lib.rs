//! The browser's collaboration link for DeckCraft.
//!
//! The cabinet opens a session through the host bridge (`deckcraft:session`, the `session` message
//! of `craft-web-host`, ADR-0016 §3). This crate turns that message into the same
//! [`deckcraft_collab::CollabDoc`] the desktop app uses and dials the sync server with the browser's
//! `WebSocket`; only the transport differs. Nothing here reaches the network without a session.
//!
//! - The token goes in the first frame and in the media requests' `Authorization` header, never in
//!   a URL, a log line or a DOM attribute ([`HostSession`]'s `Debug` prints `<redacted>`).
//! - Media never travels inside an operation: the link fetches a picture's bytes before it applies
//!   the command that names them, and holds the message until they are here
//!   ([`media::FrameQueue`]).
//! - The protocol half is transport-free: [`WebLink::on_text`] takes a server frame as a string, so
//!   the session is tested headlessly with an in-process [`deckcraft_collab::Hub`] on the other side.
//!
//! The messages themselves and their `post_*` calls are `craft-web-host`'s; this crate is
//! DeckCraft's side of the contract (`INTEGRATION.md` §6).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod host;
pub mod media;

#[cfg(target_arch = "wasm32")]
mod socket;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

#[cfg(target_arch = "wasm32")]
use deckcraft_collab::CollabDoc;
use deckcraft_collab::Transport;
#[cfg(target_arch = "wasm32")]
use deckcraft_collab_proto::ClientInfo;
use deckcraft_collab_proto::{ClientMsg, ServerMsg, decode_server};
#[cfg(target_arch = "wasm32")]
use deckcraft_engine::Session;

pub use deckcraft_collab::color_for;
pub use host::{HostAction, HostEvent, HostParticipant, HostSession, HostState, host_participants, role_word};
pub use media::{FrameQueue, MEDIA_WAIT_MS, MediaUrls};

/// Bytes of media objects this replica has: the digest the operation names, and the object.
type Cache = Arc<Mutex<HashMap<String, Vec<u8>>>>;

fn cache_get(cache: &Cache, sha: &str) -> Option<Vec<u8>> {
    cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(sha).cloned()
}

pub(crate) fn cache_put(cache: &Cache, sha: &str, bytes: Vec<u8>) {
    cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(sha.to_string(), bytes);
}

/// The other end of a transport-free link: what it sent, and what the server answered. A page's
/// socket is not involved — the test hands the messages to an in-process server itself.
#[derive(Clone, Default, Debug)]
pub struct Wire {
    sent: Sent,
    incoming: Rc<RefCell<std::collections::VecDeque<String>>>,
}

/// What a transport-free link was asked to send, in order.
#[derive(Clone, Default, Debug)]
struct Sent(Rc<RefCell<Vec<ClientMsg>>>);

impl Wire {
    /// Everything the app sent since the last call, in order.
    pub fn sent(&self) -> Vec<ClientMsg> {
        std::mem::take(&mut *self.sent.0.borrow_mut())
    }

    /// Hand one server frame to the link; it is delivered on the next poll, as a socket's frame
    /// would be.
    pub fn deliver(&self, text: &str) {
        self.incoming.borrow_mut().push_back(text.to_string());
    }

    /// Frames the link has not taken yet.
    pub fn pending(&self) -> usize {
        self.incoming.borrow().len()
    }
}

/// The collaboration link the browser build dials.
///
/// It moves bytes and media; the session state (queue, versions, resync) stays in `CollabDoc`, as
/// on the desktop.
pub struct WebLink {
    /// The session's HTTP endpoints, when its URL names them.
    urls: Option<MediaUrls>,
    /// The handshake this link sends first on every connection.
    #[cfg_attr(
        not(target_arch = "wasm32"),
        allow(dead_code, reason = "a page's socket re-sends it on every connect; a test link records it once and never re-sends it")
    )]
    hello: ClientMsg,
    /// Server frames, each waiting for the media it names.
    queue: FrameQueue,
    /// Media bytes this replica holds, shared with the fetcher `CollabDoc` calls.
    cache: Cache,
    /// Digests a fetch was already started for.
    #[cfg_attr(
        not(target_arch = "wasm32"),
        allow(dead_code, reason = "a page's link fetches media itself; a test serves it from the link's own cache")
    )]
    requested: HashSet<String>,
    /// Native only: what the app sent, for a test's in-process server.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code, reason = "a page's socket carries what the app sent; only a test's link records it"))]
    sent: Option<Wire>,
    /// Whether the socket is up. A native test link starts connected to its in-process server.
    open: bool,
    /// The clock a media wait is measured with on targets without a browser.
    clock_ms: f64,
    #[cfg(target_arch = "wasm32")]
    socket: Option<socket::Transport>,
    #[cfg(target_arch = "wasm32")]
    wake: Rc<dyn Fn()>,
}

impl std::fmt::Debug for WebLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebLink").field("urls", &self.urls).field("open", &self.open).field("queued", &self.queue).finish()
    }
}

/// Build the client and its link for one host session (wasm32): the document the app has open, the
/// token, and the page's socket. `wake` is called when the socket has something for the next frame
/// (eframe only redraws on demand).
#[cfg(target_arch = "wasm32")]
pub fn connect(session: &HostSession, engine: &Session, wake: impl Fn() + 'static) -> Result<(CollabDoc, WebLink), String> {
    let mut doc = CollabDoc::new(session.session_id());
    let info = ClientInfo { name: "DeckCraft".into(), ver: env!("CARGO_PKG_VERSION").into() };
    let hello = doc.connect(engine, session.token().to_string(), info);
    let link = WebLink::open(session, hello, wake)?;
    // A replica that is not the picture's author needs its bytes before the command can run; the
    // link fetches them first, so this normally hits the cache.
    let cache = link.cache.clone();
    doc.set_media_fetcher(Arc::new(move |sha: &str| cache_get(&cache, sha)));
    Ok((doc, link))
}

/// A link without a socket: the tests feed it the server's frames and read what it sent. No network
/// call is made, and the handshake is its first recorded message.
pub fn offline(session: &HostSession, hello: ClientMsg) -> (WebLink, Wire) {
    let wire = Wire::default();
    wire.sent.0.borrow_mut().push(hello.clone());
    let link = WebLink {
        urls: MediaUrls::from_session(session),
        hello,
        queue: FrameQueue::default(),
        cache: Cache::default(),
        requested: HashSet::new(),
        sent: Some(wire.clone()),
        open: true,
        clock_ms: 0.0,
        #[cfg(target_arch = "wasm32")]
        socket: None,
        #[cfg(target_arch = "wasm32")]
        wake: Rc::new(|| {}),
    };
    (link, wire)
}

impl WebLink {
    /// Dial the session's URL. The token goes in the handshake, never in the URL.
    #[cfg(target_arch = "wasm32")]
    pub fn open(session: &HostSession, hello: ClientMsg, wake: impl Fn() + 'static) -> Result<WebLink, String> {
        let wake: Rc<dyn Fn()> = Rc::new(wake);
        let socket = socket::Transport::open(session.ws_url(), wake.clone())?;
        Ok(WebLink {
            urls: MediaUrls::from_session(session),
            hello,
            queue: FrameQueue::default(),
            cache: Cache::default(),
            requested: HashSet::new(),
            sent: None,
            open: false,
            clock_ms: 0.0,
            socket: Some(socket),
            wake,
        })
    }

    /// Handle one server frame, transport-free: the entry point a test drives.
    pub fn on_text(&mut self, text: &str) -> bool {
        self.on_text_at(text, self.now_ms())
    }

    /// Handle one server frame at `now_ms` (the clock the media wait is measured with).
    pub fn on_text_at(&mut self, text: &str, now_ms: f64) -> bool {
        let Ok(msg) = decode_server(text) else { return false };
        self.push(msg, now_ms);
        true
    }

    /// Queue one decoded frame and start the fetches it needs.
    pub fn push(&mut self, msg: ServerMsg, now_ms: f64) {
        let cache = self.cache.clone();
        self.queue.push(msg, move |sha| cache_get(&cache, sha).is_some(), now_ms);
        self.start_fetches();
    }

    /// Ask for the bytes of every frame the queue is holding.
    fn start_fetches(&mut self) {
        #[cfg(target_arch = "wasm32")]
        for sha in self.queue.waiting() {
            if !self.requested.insert(sha.clone()) {
                continue;
            }
            let Some(urls) = &self.urls else { continue };
            socket::fetch(urls, &sha, self.cache.clone(), self.wake.clone());
        }
    }

    /// Whether the socket is up right now.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The platform clock, in milliseconds: the browser's, or the test's.
    fn now_ms(&self) -> f64 {
        #[cfg(target_arch = "wasm32")]
        {
            js_sys::Date::now()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.clock_ms
        }
    }
}

impl Transport for WebLink {
    fn send(&mut self, msg: ClientMsg) -> Result<(), String> {
        #[cfg(target_arch = "wasm32")]
        {
            let text = deckcraft_collab_proto::encode_client(&msg).map_err(|err| format!("cannot encode a collaboration message: {err}"))?;
            match &self.socket {
                Some(socket) => socket.send(&text),
                None => Err("this link has no socket".into()),
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            match &self.sent {
                Some(wire) => {
                    wire.sent.0.borrow_mut().push(msg);
                    Ok(())
                }
                None => Err("the web transport is not available on this target".into()),
            }
        }
    }

    fn next(&mut self) -> Option<ServerMsg> {
        #[cfg(target_arch = "wasm32")]
        {
            let now = js_sys::Date::now();
            let incoming = match &self.socket {
                Some(socket) => socket.poll(),
                None => Vec::new(),
            };
            for event in incoming {
                match event {
                    socket::Incoming::Opened => {
                        self.open = true;
                        let hello = self.hello.clone();
                        if let Err(error) = self.send(hello) {
                            log::warn!("collab handshake: {error}");
                        }
                    }
                    socket::Incoming::Text(text) => {
                        self.on_text_at(&text, now);
                    }
                    socket::Incoming::Closed => self.open = false,
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            // A test's frames arrive through the wire, as a socket's would.
            if let Some(wire) = &self.sent {
                let frames: Vec<String> = wire.incoming.borrow_mut().drain(..).collect();
                let now = self.now_ms();
                for text in frames {
                    self.on_text_at(&text, now);
                }
            }
        }
        let now = self.now_ms();
        let cache = self.cache.clone();
        self.queue.next(move |sha| cache_get(&cache, sha).is_some(), now)
    }

    fn is_open(&self) -> bool {
        self.open
    }

    fn put_media(&mut self, sha: &str, bytes: &[u8]) -> Result<(), String> {
        if !deckcraft_collab::valid_sha(sha) {
            return Err("the media name is not a sha256 digest".into());
        }
        cache_put(&self.cache, sha, bytes.to_vec());
        #[cfg(target_arch = "wasm32")]
        {
            // The upload is a `fetch`, which a browser runs asynchronously: the operation is
            // already queued, and an upload that never lands ends in the server's `rejected`
            // (`missing_media`), not in a divergence.
            let urls = self.urls.clone().ok_or("this session's URL names no media endpoint")?;
            socket::upload(&urls, sha, bytes, self.wake.clone());
            Ok(())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Err("the browser media upload is not available on this target".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> HostSession {
        HostSession::new("s-1", "ws://127.0.0.1:8080/v1/sessions/s-1/ws", "jwt", "editor")
    }

    #[test]
    fn the_handshake_is_the_first_frame_and_the_token_stays_out_of_the_url() {
        let (mut link, wire) = offline(&session(), ClientMsg::Ping);
        assert!(matches!(wire.sent().as_slice(), [ClientMsg::Ping]));
        link.send(ClientMsg::Undo).expect("the test link records");
        assert!(matches!(wire.sent().as_slice(), [ClientMsg::Undo]));
        assert!(link.is_open(), "a test link is up to its in-process server");
    }

    #[test]
    fn a_frame_that_is_not_a_server_message_is_ignored() {
        let (mut link, _wire) = offline(&session(), ClientMsg::Ping);
        assert!(!link.on_text("{not json"));
        assert!(link.on_text(&serde_json::to_string(&ServerMsg::Pong).expect("encode")));
    }

    #[test]
    fn a_delivered_frame_reaches_the_app_through_the_link() {
        let (mut link, wire) = offline(&session(), ClientMsg::Ping);
        wire.deliver(&serde_json::to_string(&ServerMsg::Pong).expect("encode"));
        assert_eq!(wire.pending(), 1);
        assert!(matches!(link.next(), Some(ServerMsg::Pong)));
        assert_eq!(wire.pending(), 0);
    }
}
