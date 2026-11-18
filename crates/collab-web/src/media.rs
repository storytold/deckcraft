//! The media side of the browser link: where the session's pictures live, which digests a server
//! frame needs before it can be applied, and the queue that holds such a frame until the bytes are
//! here.
//!
//! Media never travels inside an operation (`INTEGRATION.md`, "Медиа… не должны пересылаться в
//! каждой операции"): the operation carries a `sha256`, the bytes go over HTTP. The desktop link
//! fetches them synchronously (`GET /media/{sha256}`) before it applies the command; a browser
//! cannot, so the web link asks for them first and holds the frame. A frame whose bytes never
//! arrive is released after [`MEDIA_WAIT_MS`], and the adapter then reports the picture it cannot
//! place — never a silent divergence.

use std::collections::VecDeque;
use std::fmt;

use deckcraft_collab::valid_sha;
use deckcraft_collab_proto::{Payload, ServerMsg};

use crate::host::HostSession;

/// How long a frame waits for the media it names before it is applied without them.
pub const MEDIA_WAIT_MS: f64 = 10_000.0;

/// The HTTP endpoints of one session, derived from its WebSocket URL exactly as the desktop link
/// derives them: `ws://host:8080/v1/sessions/{id}/ws` → `http://host:8080`, uploads under
/// `/v1/sessions/{id}/media/{sha256}`, fetches under `/media/{sha256}`. A `wss://` URL becomes
/// `https://`.
#[derive(Clone)]
pub struct MediaUrls {
    /// `http://host:port` or `https://host:port`, without a trailing slash.
    base: String,
    session_id: String,
    token: String,
}

impl MediaUrls {
    /// The endpoints a session's URL names, or `None` when it is not a session endpoint (no
    /// `ws://`/`wss://` scheme, no authority, or a path that is not
    /// `/v1/sessions/{id}/ws`).
    pub fn from_session(session: &HostSession) -> Option<MediaUrls> {
        let url = session.ws_url();
        let (scheme, rest) = match url.strip_prefix("wss://") {
            Some(rest) => ("https://", rest),
            None => ("http://", url.strip_prefix("ws://")?),
        };
        let (authority, path) = match rest.split_once('/') {
            Some((authority, path)) => (authority, path),
            None => (rest, ""),
        };
        if authority.is_empty() || session.token().is_empty() {
            return None;
        }
        let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
        let session_id = match segments.as_slice() {
            ["v1", "sessions", id, "ws"] => (*id).to_string(),
            _ => return None,
        };
        Some(MediaUrls { base: format!("{scheme}{authority}"), session_id, token: session.token().to_string() })
    }

    /// `GET` URL of one object.
    pub fn fetch(&self, sha: &str) -> String {
        format!("{}/media/{sha}", self.base)
    }

    /// `PUT` URL of one object.
    pub fn upload(&self, sha: &str) -> String {
        format!("{}/v1/sessions/{}/media/{sha}", self.base, self.session_id)
    }

    /// The token for the `Authorization` header. Never put it in a URL.
    pub fn bearer(&self) -> String {
        format!("Bearer {}", self.token)
    }
}

impl fmt::Debug for MediaUrls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MediaUrls").field("base", &self.base).field("session_id", &self.session_id).field("token", &"<redacted>").finish()
    }
}

/// The media digests a frame cannot be applied without: a confirmed picture, video or audio insert
/// names its bytes by `sha256`; the snapshot tail replays such operations one by one.
pub fn wanted(msg: &ServerMsg) -> Vec<String> {
    let mut out = Vec::new();
    match msg {
        ServerMsg::Applied(applied) => {
            if let Some(command) = &applied.command {
                push_digest(&command.payload, &mut out);
            }
        }
        ServerMsg::Snapshot(snapshot) => {
            for applied in &snapshot.tail {
                if let Some(command) = &applied.command {
                    push_digest(&command.payload, &mut out);
                }
            }
        }
        _ => {}
    }
    out.retain(|sha| valid_sha(sha));
    out
}

fn push_digest(payload: &Payload, out: &mut Vec<String>) {
    match payload {
        Payload::PictureInsert { sha256, .. } | Payload::MediaInsert { sha256, .. } | Payload::FontCreate { sha256, .. } => out.push(sha256.clone()),
        _ => {}
    }
}

/// Frames in arrival order, each waiting for the media it names (if any).
///
/// The link delivers them through [`FrameQueue::next`], which stops at the first frame whose bytes
/// are not here yet: a later frame applied first would move the version past it.
#[derive(Default, Debug)]
pub struct FrameQueue {
    queue: VecDeque<Held>,
}

#[derive(Debug)]
enum Held {
    Ready(ServerMsg),
    Waiting { msg: ServerMsg, sha: String, since_ms: f64 },
}

impl FrameQueue {
    /// Queue one frame. `have` answers whether the replica already holds a digest; `now_ms` is
    /// the clock the wait is measured with.
    pub fn push(&mut self, msg: ServerMsg, have: impl Fn(&str) -> bool, now_ms: f64) {
        let missing = wanted(&msg).into_iter().find(|sha| !have(sha));
        match missing {
            Some(sha) => self.queue.push_back(Held::Waiting { msg, sha, since_ms: now_ms }),
            None => self.queue.push_back(Held::Ready(msg)),
        }
    }

    /// The next frame that may be applied, in order.
    pub fn next(&mut self, have: impl Fn(&str) -> bool, now_ms: f64) -> Option<ServerMsg> {
        let ready = match self.queue.front()? {
            Held::Ready(_) => true,
            Held::Waiting { sha, since_ms, .. } => have(sha) || now_ms - since_ms >= MEDIA_WAIT_MS,
        };
        if !ready {
            return None;
        }
        match self.queue.pop_front()? {
            Held::Ready(msg) | Held::Waiting { msg, .. } => Some(msg),
        }
    }

    /// Digests the queue is holding frames for, without duplicates: what the link still has to
    /// fetch.
    pub fn waiting(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for held in &self.queue {
            if let Held::Waiting { sha, .. } = held
                && !out.contains(sha)
            {
                out.push(sha.clone());
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_collab_proto::{Applied, ConfirmedCommand, Target};

    fn session() -> MediaUrls {
        let session = HostSession::new("s-7", "wss://collab.example/v1/sessions/s-7/ws", "jwt", "editor");
        MediaUrls::from_session(&session).expect("a session endpoint")
    }

    fn picture(sha: &str, version: u64) -> ServerMsg {
        ServerMsg::Applied(Applied {
            op_id: format!("op-{version}"),
            version,
            actor_id: "alice@example.com".into(),
            command: Some(ConfirmedCommand {
                cmd: "insert.picture".into(),
                target: Target::default(),
                payload: Payload::PictureInsert { sha256: sha.into(), name: "red.png".into(), rect: None },
            }),
            delta: Default::default(),
        })
    }

    fn sha() -> String {
        deckcraft_collab::sha256_hex(b"a picture")
    }

    #[test]
    fn the_endpoints_come_from_the_sockets_url_and_a_plain_socket_uses_http() {
        assert_eq!(session().fetch("abc"), "https://collab.example/media/abc");
        assert_eq!(session().upload("abc"), "https://collab.example/v1/sessions/s-7/media/abc");
        let plain = HostSession::new("s-1", "ws://127.0.0.1:8080/v1/sessions/s-1/ws", "t", "editor");
        let urls = MediaUrls::from_session(&plain).expect("plain");
        assert_eq!(urls.fetch("abc"), "http://127.0.0.1:8080/media/abc");
    }

    #[test]
    fn a_url_that_is_not_a_session_endpoint_has_no_media() {
        for url in ["https://collab.example/ws", "ws:///v1/sessions/s/ws", "ws://host/v1/sessions/ws", "ws://host/media/x"] {
            let fixture = HostSession::new("s", url, "t", "editor");
            assert!(MediaUrls::from_session(&fixture).is_none(), "{url}");
        }
        let no_token = HostSession::new("s", "ws://host/v1/sessions/s/ws", "", "editor");
        assert!(MediaUrls::from_session(&no_token).is_none(), "a request without a token is refused by the server");
    }

    #[test]
    fn the_token_is_redacted_and_never_part_of_a_url() {
        let urls = session();
        let debug = format!("{urls:?}");
        assert!(!debug.contains("jwt"), "{debug}");
        assert!(debug.contains("<redacted>"), "{debug}");
        assert!(!urls.fetch("abc").contains("jwt"));
        assert!(!urls.upload("abc").contains("jwt"));
        assert_eq!(urls.bearer(), "Bearer jwt");
    }

    #[test]
    fn only_media_operations_name_digests() {
        assert_eq!(wanted(&picture("a".repeat(64).as_str(), 1)), vec!["a".repeat(64)]);
        assert!(wanted(&ServerMsg::Pong).is_empty());
        // A digest that is not a sha256 is not fetched: the server would refuse it anyway.
        assert!(wanted(&picture("not-a-digest", 1)).is_empty());
    }

    #[test]
    fn a_frame_waits_for_its_bytes_and_a_later_one_never_overtakes_it() {
        let sha = sha();
        let mut queue = FrameQueue::default();
        queue.push(picture(&sha, 1), |_| false, 0.0);
        queue.push(ServerMsg::Pong, |_| true, 0.0);
        assert_eq!(queue.waiting(), vec![sha.clone()]);
        assert!(queue.next(|_| false, 0.0).is_none(), "the picture is still missing, so nothing may be applied");
        let mut held = Vec::new();
        while let Some(msg) = queue.next(|asked| asked == sha, 1_000.0) {
            held.push(msg);
        }
        assert_eq!(held.len(), 2, "the picture then the frame behind it, in order");
        assert!(matches!(held[0], ServerMsg::Applied(_)));
        assert!(queue.is_empty());
    }

    #[test]
    fn a_frame_without_media_needs_no_wait() {
        let mut queue = FrameQueue::default();
        queue.push(ServerMsg::Pong, |_| false, 0.0);
        assert!(queue.waiting().is_empty());
        assert!(matches!(queue.next(|_| false, 0.0), Some(ServerMsg::Pong)));
    }

    #[test]
    fn a_picture_that_never_arrives_is_applied_after_the_wait() {
        let sha = sha();
        let mut queue = FrameQueue::default();
        queue.push(picture(&sha, 1), |_| false, 0.0);
        assert!(queue.next(|_| false, MEDIA_WAIT_MS - 1.0).is_none());
        assert!(matches!(queue.next(|_| false, MEDIA_WAIT_MS), Some(ServerMsg::Applied(_))));
    }
}
