//! The desktop WebSocket link: one background thread, two channels, and a read timeout so the
//! UI thread can push messages without waiting for the socket.
//!
//! The link moves bytes only. It reconnects and resends the handshake, but the session state
//! (queue, versions, resync) stays in [`crate::ClientSession`]; a dropped socket is reported as
//! `ServerMsg::Error { code: "offline" }`, and the application decides when to retry.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, LazyLock};
use std::thread;
use std::time::Duration;

use deckcraft_collab_proto::{ClientMsg, ServerMsg, decode_server, encode_client};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

use crate::Transport;
use tungstenite::error::Error as WsError;
use tungstenite::stream::MaybeTlsStream;

/// The process-wide TLS client configuration, built from the platform's root store on first use.
/// `None` when no roots could be loaded: the link then reports why a `wss://` media request
/// cannot be made instead of pretending there is no certificate problem.
static TLS: LazyLock<Option<Arc<ClientConfig>>> = LazyLock::new(|| {
    let mut roots = RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().certs {
        let _ = roots.add(cert);
    }
    if roots.is_empty() {
        return None;
    }
    Some(Arc::new(ClientConfig::builder().with_root_certificates(roots).with_no_client_auth()))
});

/// A desktop WebSocket client with a background reader/writer.
pub struct WsTransport {
    out: Sender<ClientMsg>,
    incoming: Receiver<ServerMsg>,
    open: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
    /// The HTTP side of the same server, for media: plain for `ws://`, TLS for `wss://`.
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
        let media = Endpoint::parse(&url, &jwt);
        let thread_url = url;
        let thread_open = open.clone();
        let thread_stop = stop.clone();
        let join = thread::spawn(move || run(&thread_url, hello, out_rx, in_tx, thread_open, thread_stop));
        WsTransport { out: out_tx, incoming: in_rx, open, stop, join: Some(join), media }
    }

    /// A fetcher for media someone else inserted, for [`crate::CollabDoc::set_media_fetcher`].
    /// Works for `ws://` and `wss://`; `None` when the URL names no media endpoint.
    pub fn media_fetcher(url: &str, jwt: &str) -> Option<Arc<dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync>> {
        let endpoint = Endpoint::parse(url, jwt)?;
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
            return Err("this link has no media endpoint; use a ws:// or wss:// session URL".into());
        };
        if !crate::valid_sha(sha) {
            return Err("the media name is not a sha256 digest".into());
        }
        media.put_media(sha, bytes)
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

/// The HTTP side of one server: requests to the host the WebSocket URL names. `ws://` uses a
/// plain socket, `wss://` a TLS connection verified against the platform root store.
#[derive(Clone, Debug)]
struct Endpoint {
    scheme: Scheme,
    /// Host name for the TLS handshake and the `Host` header.
    host: String,
    /// `host:port` for `TcpStream::connect`.
    authority: String,
    /// Value of the `Host` header (the port is omitted when it is the scheme's default).
    host_header: String,
    session: String,
    token: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scheme {
    Plain,
    Tls,
}

impl Scheme {
    fn default_port(self) -> u16 {
        match self {
            Scheme::Plain => 80,
            Scheme::Tls => 443,
        }
    }
}

impl Endpoint {
    fn parse(ws_url: &str, jwt: &str) -> Option<Self> {
        let (scheme, rest) = match ws_url.strip_prefix("wss://") {
            Some(rest) => (Scheme::Tls, rest),
            None => (Scheme::Plain, ws_url.strip_prefix("ws://")?),
        };
        let (authority, path) = match rest.split_once('/') {
            Some((authority, path)) => (authority, path),
            None => (rest, ""),
        };
        if authority.is_empty() || jwt.is_empty() {
            return None;
        }
        let (host, port) = split_host_port(authority)?;
        let port = port.unwrap_or(scheme.default_port());
        let path: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        // `/v1/sessions/{id}/ws`, or a bare `ws://host` for a fetcher that only needs `GET`.
        let session = match path.as_slice() {
            ["v1", "sessions", id, "ws"] => (*id).to_string(),
            [] => String::new(),
            _ => return None,
        };
        let authority = if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") };
        let host_header =
            if port == scheme.default_port() { if host.contains(':') { format!("[{host}]") } else { host.clone() } } else { authority.clone() };
        Some(Endpoint { scheme, host, authority, host_header, session, token: jwt.to_string() })
    }

    fn connect(&self) -> Result<Wire, String> {
        let socket = TcpStream::connect(&self.authority).map_err(|e| format!("media: {e}"))?;
        socket.set_read_timeout(Some(Duration::from_secs(10))).ok();
        socket.set_write_timeout(Some(Duration::from_secs(10))).ok();
        match self.scheme {
            Scheme::Plain => Ok(Wire::Plain(socket)),
            Scheme::Tls => {
                let config = TLS.as_ref().ok_or("media: no TLS root certificates are available")?.clone();
                let name = ServerName::try_from(self.host.clone()).map_err(|e| format!("media: {e}"))?;
                let connection = ClientConnection::new(config, name).map_err(|e| format!("media: {e}"))?;
                Ok(Wire::Tls(Box::new(StreamOwned::new(connection, socket))))
            }
        }
    }

    fn request(&self, method: &str, path: &str, body: &[u8]) -> Result<(u16, Vec<u8>), String> {
        let mut stream = self.connect()?;
        let head = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.host_header,
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

    fn put_media(&self, sha: &str, bytes: &[u8]) -> Result<(), String> {
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

/// One HTTP connection: plain, or TLS over the same socket.
enum Wire {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl Read for Wire {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Wire::Plain(s) => s.read(buf),
            Wire::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Wire {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Wire::Plain(s) => s.write(buf),
            Wire::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Wire::Plain(s) => s.flush(),
            Wire::Tls(s) => s.flush(),
        }
    }
}

/// `host`, `host:port`, `[::1]` or `[::1]:8080`. A bare IPv6 address without brackets has no
/// unambiguous port and is refused.
fn split_host_port(authority: &str) -> Option<(String, Option<u16>)> {
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, tail) = rest.split_once(']')?;
        if host.is_empty() {
            return None;
        }
        return match tail.strip_prefix(':') {
            Some(port) => Some((host.to_string(), Some(port.parse().ok()?))),
            None if tail.is_empty() => Some((host.to_string(), None)),
            None => None,
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => Some((host.to_string(), Some(port.parse().ok()?))),
        Some(_) => None,
        None if !authority.is_empty() => Some((authority.to_string(), None)),
        None => None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ws_url_names_the_session_and_the_plain_endpoint() {
        let e = Endpoint::parse("ws://127.0.0.1:8080/v1/sessions/demo/ws", "jwt").expect("endpoint");
        assert_eq!(e.scheme, Scheme::Plain);
        assert_eq!(e.authority, "127.0.0.1:8080");
        assert_eq!(e.host_header, "127.0.0.1:8080");
        assert_eq!(e.session, "demo");
    }

    #[test]
    fn a_wss_url_uses_tls_and_the_default_port() {
        let e = Endpoint::parse("wss://collab.example.com/v1/sessions/demo/ws", "jwt").expect("endpoint");
        assert_eq!(e.scheme, Scheme::Tls);
        assert_eq!(e.authority, "collab.example.com:443");
        assert_eq!(e.host_header, "collab.example.com", "the default port stays out of Host");
        assert_eq!(e.session, "demo");
    }

    #[test]
    fn a_bare_host_is_a_fetcher_endpoint_and_nothing_else() {
        let e = Endpoint::parse("wss://collab.example.com:8443", "jwt").expect("endpoint");
        assert_eq!(e.session, "");
        assert_eq!(e.host_header, "collab.example.com:8443");
        assert!(Endpoint::parse("https://collab.example.com", "jwt").is_none(), "only ws and wss spell the server");
        assert!(Endpoint::parse("ws://collab.example.com/v1/sessions/demo/ws", "").is_none(), "a token is required");
    }

    #[test]
    fn ipv6_hosts_keep_their_brackets_out_of_the_tls_name() {
        let e = Endpoint::parse("wss://[::1]:9443/v1/sessions/demo/ws", "jwt").expect("endpoint");
        assert_eq!(e.host, "::1");
        assert_eq!(e.authority, "[::1]:9443");
        assert_eq!(e.host_header, "[::1]:9443");
    }
}
