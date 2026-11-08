//! The desktop WebSocket link: one background thread, two channels, and a read timeout so the
//! UI thread can push messages without waiting for the socket.
//!
//! The link moves bytes only. It reconnects and resends the handshake, but the session state
//! (queue, versions, resync) stays in [`crate::ClientSession`]; a dropped socket is reported as
//! `ServerMsg::Error { code: "offline" }`, and the application decides when to retry.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread;
use std::time::Duration;

use deckcraft_collab_proto::{ClientMsg, ServerMsg, decode_server, encode_client};

use crate::Transport;
use tungstenite::error::Error as WsError;
use tungstenite::stream::MaybeTlsStream;

/// A desktop WebSocket client with a background reader/writer.
pub struct WsTransport {
    out: Sender<ClientMsg>,
    incoming: Receiver<ServerMsg>,
    open: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
    /// The plain-HTTP side of the same server, for pictures. `None` for a TLS link until the
    /// host supplies a TLS HTTP client.
    media: Option<Endpoint>,
}

impl WsTransport {
    /// Connect to `url` and send `hello` first; the thread reconnects with the same handshake and
    /// keeps unread messages in the channel while the UI is busy.
    pub fn start(url: impl Into<String>, hello: ClientMsg) -> Self {
        let (out_tx, out_rx) = channel::<ClientMsg>();
        let (in_tx, in_rx) = channel::<ServerMsg>();
        let open = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let url = url.into();
        let jwt = match &hello {
            ClientMsg::Hello(hello) => hello.jwt.clone(),
            _ => String::new(),
        };
        let media = Endpoint::plain(&url, &jwt);
        let thread_url = url;
        let thread_open = open.clone();
        let thread_stop = stop.clone();
        let join = thread::spawn(move || run(&thread_url, hello, out_rx, in_tx, thread_open, thread_stop));
        WsTransport { out: out_tx, incoming: in_rx, open, stop, join: Some(join), media }
    }

    /// A fetcher for pictures someone else inserted, for [`crate::CollabDoc::set_media_fetcher`].
    /// Plain `ws://` only, like [`Self::put_media`].
    pub fn media_fetcher(url: &str, jwt: &str) -> Option<Arc<dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync>> {
        let endpoint = Endpoint::plain(url, jwt)?;
        Some(Arc::new(move |sha: &str| {
            if !crate::valid_sha(sha) {
                return None;
            }
            let (status, body) = endpoint.request("GET", &format!("/media/{sha}"), &[]).ok()?;
            (status == 200).then_some(body)
        }))
    }
}

impl Transport for WsTransport {
    fn send(&mut self, msg: ClientMsg) -> Result<(), String> {
        self.out.send(msg).map_err(|e| e.to_string())
    }

    fn next(&mut self) -> Option<ServerMsg> {
        self.incoming.try_recv().ok()
    }

    fn is_open(&self) -> bool {
        self.open.load(Ordering::Relaxed)
    }

    fn put_media(&mut self, sha: &str, bytes: &[u8]) -> Result<(), String> {
        let Some(media) = &self.media else {
            return Err("picture uploads need a plain ws:// link; this link has no media channel".into());
        };
        if !crate::valid_sha(sha) {
            return Err("the picture name is not a sha256 digest".into());
        }
        media.put_picture(sha, bytes)
    }
}

impl Drop for WsTransport {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            // The reader loop wakes on its 50 ms timeout; don't block the UI if it is mid-connect.
            let _ = join.join();
        }
    }
}

/// The plain-HTTP side of one server: `http://` requests to the host the WebSocket URL names.
/// Only `ws://` URLs describe it; a `wss://` link needs a TLS client the host supplies.
#[derive(Clone, Debug)]
struct Endpoint {
    /// `host:port` for `TcpStream::connect` and the `Host` header.
    authority: String,
    session: String,
    token: String,
}

impl Endpoint {
    fn plain(ws_url: &str, jwt: &str) -> Option<Self> {
        let rest = ws_url.strip_prefix("ws://")?;
        let mut parts = rest.split('/');
        let authority = parts.next().filter(|a| !a.is_empty())?.to_string();
        let path: Vec<&str> = parts.filter(|p| !p.is_empty()).collect();
        // `/v1/sessions/{id}/ws`, or a bare `ws://host` for a fetcher that only needs `GET`.
        let session = match path.as_slice() {
            ["v1", "sessions", id, "ws"] => (*id).to_string(),
            [] => String::new(),
            _ => return None,
        };
        if jwt.is_empty() {
            return None;
        }
        Some(Endpoint { authority, session, token: jwt.to_string() })
    }

    fn request(&self, method: &str, path: &str, body: &[u8]) -> Result<(u16, Vec<u8>), String> {
        let mut stream = TcpStream::connect(&self.authority).map_err(|e| format!("media: {e}"))?;
        stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
        stream.set_write_timeout(Some(Duration::from_secs(10))).ok();
        let head = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.authority,
            self.token,
            body.len()
        );
        stream.write_all(head.as_bytes()).map_err(|e| format!("media: {e}"))?;
        if !body.is_empty() {
            stream.write_all(body).map_err(|e| format!("media: {e}"))?;
        }
        stream.flush().map_err(|e| format!("media: {e}"))?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).map_err(|e| format!("media: {e}"))?;
        let split = response.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4).ok_or("media: the server sent no headers")?;
        let status =
            String::from_utf8_lossy(&response[..split]).split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or("media: no status line")?;
        Ok((status, response[split..].to_vec()))
    }

    fn put_picture(&self, sha: &str, bytes: &[u8]) -> Result<(), String> {
        if self.session.is_empty() {
            return Err("the media link does not name a session".into());
        }
        let (status, body) = self.request("PUT", &format!("/v1/sessions/{}/media/{sha}", self.session), bytes)?;
        if matches!(status, 200 | 201) {
            return Ok(());
        }
        Err(format!("media upload: HTTP {status} {}", String::from_utf8_lossy(&body)))
    }
}

fn run(url: &str, hello: ClientMsg, out: Receiver<ClientMsg>, incoming: Sender<ServerMsg>, open: Arc<AtomicBool>, stop: Arc<AtomicBool>) {
    let mut backoff = Duration::from_millis(200);
    while !stop.load(Ordering::Relaxed) {
        match tungstenite::connect(url) {
            Ok((mut ws, _)) => {
                if set_timeout(&mut ws, Duration::from_millis(50)).is_err() {
                    let _ = incoming.send(offline("cannot set a socket timeout"));
                    break;
                }
                if send_json(&mut ws, &hello).is_err() {
                    let _ = incoming.send(offline("the handshake could not be sent"));
                    thread::sleep(backoff);
                    continue;
                }
                open.store(true, Ordering::Relaxed);
                backoff = Duration::from_millis(200);
                loop {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    // Messages from the UI first, so a command is not starved by an idle socket.
                    loop {
                        match out.try_recv() {
                            Ok(msg) => {
                                if send_json(&mut ws, &msg).is_err() {
                                    break;
                                }
                            }
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => {
                                stop.store(true, Ordering::Relaxed);
                                break;
                            }
                        }
                    }
                    match ws.read() {
                        Ok(tungstenite::Message::Text(text)) => {
                            if let Ok(msg) = decode_server(text.as_str()) {
                                let _ = incoming.send(msg);
                            }
                        }
                        Ok(tungstenite::Message::Ping(payload)) => {
                            let _ = ws.send(tungstenite::Message::Pong(payload));
                        }
                        Ok(tungstenite::Message::Close(_)) => break,
                        Ok(_) => {}
                        Err(WsError::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                        Err(WsError::ConnectionClosed | WsError::AlreadyClosed) => break,
                        Err(e) => {
                            log::debug!("collab ws: {e}");
                            break;
                        }
                    }
                }
                open.store(false, Ordering::Relaxed);
                let _ = incoming.send(offline("the connection was lost"));
            }
            Err(e) => {
                open.store(false, Ordering::Relaxed);
                let _ = incoming.send(offline(&format!("cannot connect: {e}")));
            }
        }
        let mut slept = Duration::ZERO;
        while slept < backoff && !stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(50));
            slept += Duration::from_millis(50);
        }
        backoff = (backoff * 2).min(Duration::from_secs(5));
    }
    open.store(false, Ordering::Relaxed);
}

fn send_json(ws: &mut tungstenite::WebSocket<MaybeTlsStream<TcpStream>>, msg: &ClientMsg) -> Result<(), String> {
    let text = encode_client(msg).map_err(|e| e.to_string())?;
    ws.send(tungstenite::Message::Text(text.into())).map_err(|e| e.to_string())
}

fn set_timeout(ws: &mut tungstenite::WebSocket<MaybeTlsStream<TcpStream>>, timeout: Duration) -> std::io::Result<()> {
    match ws.get_mut() {
        MaybeTlsStream::Plain(stream) => stream.set_read_timeout(Some(timeout)),
        MaybeTlsStream::Rustls(stream) => stream.get_ref().set_read_timeout(Some(timeout)),
        _ => Err(std::io::Error::other("unsupported stream")),
    }
}

fn offline(message: &str) -> ServerMsg {
    ServerMsg::Error(deckcraft_collab_proto::ServerError { code: "offline".into(), message: message.into(), retryable: true })
}
