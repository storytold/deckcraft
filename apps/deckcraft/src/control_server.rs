//! Loopback JSON-lines control server: one request per line, one reply per line.
//! This is the transport the MCP server (`deckcraft-cli mcp --connect`) wraps.
//!
//! The first request on a connection must be `{"method": "auth", "params": {"token": …}}` with
//! the token from `deckcraft_mcp::auth` (published to `auth::token_path(port)`); anything else,
//! including a line that isn't JSON (an HTTP request from a web page), gets an error reply and
//! closes the connection.
//!
//! Ask Agent's `agent.listen` and `agent.reply` are answered here, after that check, on the
//! connection's own thread, from the window's [`AgentHub`]: an agent waiting for the next request
//! never holds up the window or other connections.
//!
//! Every resource a client can hold is bounded ([`Limits`]): simultaneous connections, request
//! line length, queued requests, and how long a socket may sit idle or blocked on a write.

use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::time::{Duration, Instant};

use deckcraft_mcp::auth;
use deckcraft_ui_egui::{AgentHub, ControlRequest};
use serde_json::{Value, json};

const AUTH_FIRST: &str = "authenticate first: {\"method\":\"auth\",\"params\":{\"token\":…}}";

/// Resource bounds for the control server.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Connections served at once; more are told `busy` and closed.
    pub max_clients: usize,
    /// Longest request line in bytes (newline excluded); a longer one closes the connection.
    pub max_line: usize,
    /// Requests waiting for the UI at once; more are answered `busy`.
    pub queue: usize,
    /// How long a connection may go without sending a byte before it is closed.
    pub idle_timeout: Duration,
    /// How long a reply write may block before the connection is closed.
    pub write_timeout: Duration,
    /// How long a request waits for the UI's answer.
    pub reply_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_clients: 16,
            // Picture and media inserts carry base64 payloads: room for files of about 190 MB.
            max_line: 256 * 1024 * 1024,
            queue: 64,
            idle_timeout: Duration::from_secs(600),
            write_timeout: Duration::from_secs(30),
            reply_timeout: Duration::from_secs(60),
        }
    }
}

/// Start the control server on `port`; `None` when it can't (no token, port taken, token file
/// not written): the control channel, and with it Ask Agent, is then off. `agents` is the
/// window's Ask Agent queue.
pub fn start(port: u16, ctx: egui::Context, agents: AgentHub) -> Option<Receiver<ControlRequest>> {
    let token = match auth::env_token().map_or_else(auth::new_token, Ok) {
        Ok(t) => t,
        Err(e) => {
            log::error!("control server: no token: {e}");
            return None;
        }
    };
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            log::error!("control server failed to bind 127.0.0.1:{port}: {e}");
            return None;
        }
    };
    // The port actually bound (`--control 0` picks a free one).
    let port = listener.local_addr().map_or(port, |a| a.port());
    let token_file = match auth::publish_token(port, &token) {
        Ok(path) => path,
        Err(e) => {
            log::error!("control server: writing the token file: {e}");
            return None;
        }
    };
    log::info!("control server listening on 127.0.0.1:{port} (token in {})", token_file.display());
    Some(serve_on(listener, token, Limits::default(), ctx, agents))
}

/// How long the accepting thread waits after a failed accept. Accepting can keep failing, for
/// example while the process has no file descriptors left, and retrying at once would keep a
/// core busy until it recovers.
const ACCEPT_RETRY: Duration = Duration::from_millis(100);

/// The longest a connection refused as busy is drained before it is closed.
const REFUSED_GRACE: Duration = Duration::from_millis(250);

/// Refused connections drained at once, each on a thread of its own. Past this many, a refused
/// connection gets the busy reply and is closed without a drain.
const MAX_REFUSING: usize = 16;

/// The connections `incoming` accepts, calling `wait` after each failed accept.
fn accepted<T>(incoming: impl Iterator<Item = std::io::Result<T>>, mut wait: impl FnMut()) -> impl Iterator<Item = T> {
    incoming.filter_map(move |stream| stream.map_err(|_| wait()).ok())
}

/// Accept connections on `listener` under `limits`; requests arrive on the returned receiver.
fn serve_on(listener: TcpListener, token: String, limits: Limits, ctx: egui::Context, agents: AgentHub) -> Receiver<ControlRequest> {
    let (tx, rx) = sync_channel::<ControlRequest>(limits.queue.max(1));
    let token = Arc::new(token);
    let active = Arc::new(AtomicUsize::new(0));
    let refusing = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        // Each connection's number, for Ask Agent's presence.
        let mut conn = 0u64;
        for mut stream in accepted(listener.incoming(), || std::thread::sleep(ACCEPT_RETRY)) {
            let _ = stream.set_write_timeout(Some(limits.write_timeout));
            if active.fetch_add(1, Ordering::SeqCst) >= limits.max_clients {
                active.fetch_sub(1, Ordering::SeqCst);
                let _ = writeln!(stream, "{}", json!({"ok": false, "error": "busy: too many control connections"}));
                refuse(stream, &refusing, limits.write_timeout.min(REFUSED_GRACE));
                continue;
            }
            conn = conn.wrapping_add(1);
            let slot = Slot(Arc::clone(&active));
            let (tx, ctx, token, agents) = (tx.clone(), ctx.clone(), Arc::clone(&token), agents.clone());
            let spawned = std::thread::Builder::new().name("control-client".into()).spawn(move || {
                let _slot = slot;
                serve(stream, &token, &tx, &ctx, &agents, conn, limits);
            });
            if spawned.is_err() {
                log::warn!("control server could not start a client thread");
            }
        }
    });
    rx
}

/// Whether the client is still there (it hasn't closed its end of the connection): asked while
/// `agent.listen` waits and before it hands a request out, so a request never goes to a client
/// that has gone.
fn peer_open(s: &TcpStream) -> bool {
    if s.set_nonblocking(true).is_err() {
        return true;
    }
    let mut b = [0u8; 1];
    let r = s.peek(&mut b);
    let _ = s.set_nonblocking(false);
    match r {
        Ok(0) => false,
        Ok(_) => true,
        Err(e) => e.kind() == std::io::ErrorKind::WouldBlock,
    }
}

/// While an engine command waits for the UI, disconnects still release ownership promptly.
fn wait_for_ui(rx: &Receiver<Value>, state: &AtomicU8, probe: &TcpStream, agents: &AgentHub, conn: u64, limits: Limits) -> Value {
    let deadline = Instant::now() + limits.reply_timeout;
    let mut input_closed = false;
    loop {
        if !input_closed && !peer_open(probe) {
            agents.disconnect(conn);
            // Generic clients may close only their write half and still await the response.
            input_closed = true;
        }
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            // Give up on the request. While it was still queued this makes sure it never runs
            // late (#719); if the app had already taken it, its outcome is unknown.
            return if ControlRequest::abandon(state) {
                json!({"ok": false, "error": "timeout"})
            } else {
                json!({"ok": false, "error": "timeout: the app had already started the request, so it may still run; its outcome is unknown"})
            };
        };
        match rx.recv_timeout(left.min(Duration::from_millis(250))) {
            Ok(reply) => return reply,
            Err(RecvTimeoutError::Disconnected) => return json!({"ok": false, "error": "control channel closed"}),
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

/// Close a connection that was told it is refused. A peer that wrote a line before it read the
/// reply would see only a reset, so the connection is drained for up to `grace` first, on a
/// thread of its own: the accepting thread goes straight back to accepting. Past
/// [`MAX_REFUSING`] drains at once, the connection is closed without one.
fn refuse(stream: TcpStream, refusing: &Arc<AtomicUsize>, grace: Duration) {
    if refusing.fetch_add(1, Ordering::SeqCst) >= MAX_REFUSING {
        refusing.fetch_sub(1, Ordering::SeqCst);
        return;
    }
    let slot = Slot(Arc::clone(refusing));
    let spawned = std::thread::Builder::new().name("control-refused".into()).spawn(move || {
        let _slot = slot;
        if let Ok(read) = stream.try_clone() {
            close_politely(&stream, &mut BufReader::new(read), grace);
        }
    });
    if spawned.is_err() {
        log::warn!("control server could not start a thread to close a refused connection");
    }
}

/// Holds one connection slot; gives it back when the client thread ends, however it ends.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

enum Line {
    Text(Vec<u8>),
    TooLong,
    Closed,
}

/// Read one `\n`-terminated line of at most `max` bytes without buffering more than `max + 1`.
fn read_line(r: &mut impl BufRead, max: usize) -> Line {
    let mut buf = Vec::new();
    let limit = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1);
    match r.by_ref().take(limit).read_until(b'\n', &mut buf) {
        Ok(0) | Err(_) => Line::Closed,
        Ok(_) => {
            if buf.last() == Some(&b'\n') {
                buf.pop();
                Line::Text(buf)
            } else if buf.len() > max {
                Line::TooLong
            } else {
                // EOF inside a line: serve what arrived, the next read reports Closed.
                Line::Text(buf)
            }
        }
    }
}

/// The reply to a request line that arrives before the connection has authenticated, and whether
/// the connection is now authenticated (when not, it is closed).
fn authenticate(line: &str, token: &str) -> (Value, bool) {
    let Ok(msg) = serde_json::from_str::<Value>(line) else {
        return (json!({"ok": false, "error": AUTH_FIRST}), false);
    };
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let is_auth = msg.get("method").and_then(Value::as_str) == Some("auth");
    let given = msg.get("params").and_then(|p| p.get("token")).and_then(Value::as_str);
    match given {
        Some(t) if is_auth && auth::tokens_match(t, token) => (json!({"id": id, "ok": true, "result": {"authenticated": true}}), true),
        _ if is_auth => (json!({"id": id, "ok": false, "error": "wrong token"}), false),
        _ => (json!({"id": id, "ok": false, "error": AUTH_FIRST}), false),
    }
}

/// One request line, parsed.
enum Parsed {
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Blank,
    /// Not a request: reply with this error and close the connection.
    Reject(String),
}

fn parse(bytes: &[u8]) -> Parsed {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Parsed::Reject("not a control request (invalid UTF-8); closing the connection".into());
    };
    let text = text.trim();
    if text.is_empty() {
        return Parsed::Blank;
    }
    if is_http_request_line(text) {
        return Parsed::Reject("HTTP is not served here: this port takes JSON lines; closing the connection".into());
    }
    let msg = match serde_json::from_str::<Value>(text) {
        Ok(v) => v,
        Err(e) => return Parsed::Reject(format!("bad JSON ({e}); expected one JSON request object per line; closing the connection")),
    };
    let Some(obj) = msg.as_object() else {
        return Parsed::Reject("not a control request (expected a JSON object); closing the connection".into());
    };
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        return Parsed::Reject("not a control request (missing string `method`); closing the connection".into());
    };
    Parsed::Request {
        id: obj.get("id").cloned().unwrap_or(Value::Null),
        method: method.to_string(),
        params: obj.get("params").cloned().unwrap_or(json!({})),
    }
}

/// An HTTP/1 request line (`POST / HTTP/1.1`) or the HTTP/2 preface (`PRI * HTTP/2.0`).
fn is_http_request_line(text: &str) -> bool {
    const METHODS: [&str; 10] = ["GET", "HEAD", "POST", "PUT", "DELETE", "CONNECT", "OPTIONS", "TRACE", "PATCH", "PRI"];
    text.split_once(' ').is_some_and(|(method, _)| METHODS.contains(&method))
        || text.rsplit_once(' ').is_some_and(|(_, version)| version.starts_with("HTTP/"))
}

/// Close after the last reply so the peer can still read it. A peer that had sent another line
/// before the reply arrived -- two lines in one write, which any script does -- leaves input
/// unread in the socket, and closing a socket with unread input resets the connection: the reply
/// already queued for the peer is discarded and it sees only the reset. Sending the FIN first and
/// then draining, briefly and boundedly, makes the close orderly and the reason survive.
fn close_politely(out: &TcpStream, reader: &mut BufReader<TcpStream>, grace: Duration) {
    if out.shutdown(Shutdown::Write).is_err() {
        return;
    }
    // Enough for a pipelined line or two; a peer that keeps talking is dropped at the deadline.
    // The timeout is what is left of the grace before each read, not the whole of it: a peer that
    // sends a byte just before each one expires would otherwise hold this thread, and its slot,
    // for close to twice as long as the deadline says. The timeout is set on the handle that
    // reads: on Windows, a read timeout set on one handle of a `try_clone` pair can fail to apply
    // to reads through the other.
    const MAX_DRAIN: usize = 64 << 10;
    let until = Instant::now() + grace;
    let mut sink = [0u8; 4 << 10];
    let mut drained = 0;
    while drained < MAX_DRAIN {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() || reader.get_ref().set_read_timeout(Some(left)).is_err() {
            break;
        }
        match reader.read(&mut sink) {
            Ok(0) | Err(_) => break,
            Ok(n) => drained += n,
        }
    }
}

fn serve(stream: TcpStream, token: &str, tx: &SyncSender<ControlRequest>, ctx: &egui::Context, agents: &AgentHub, conn: u64, limits: Limits) {
    if stream.set_read_timeout(Some(limits.idle_timeout)).is_err() || stream.set_write_timeout(Some(limits.write_timeout)).is_err() {
        return;
    }
    let Ok(read) = stream.try_clone() else { return };
    let Ok(probe) = stream.try_clone() else {
        return;
    };
    let mut out = stream;
    let mut authenticated = false;
    let mut reader = BufReader::new(read);
    loop {
        let line = match read_line(&mut reader, limits.max_line) {
            Line::Text(t) => t,
            Line::TooLong => {
                let _ = writeln!(out, "{}", json!({"ok": false, "error": format!("request line longer than {} bytes", limits.max_line)}));
                close_politely(&out, &mut reader, limits.write_timeout);
                break;
            }
            Line::Closed => break,
        };
        let reply = match parse(&line) {
            Parsed::Blank => continue,
            Parsed::Reject(e) => {
                // The connection closes either way, but the peer is told why first: see
                // `close_politely`, without which a peer that had pipelined another line had this
                // reply reset away and saw nothing.
                let _ = writeln!(out, "{}", json!({"ok": false, "error": e}));
                close_politely(&out, &mut reader, limits.write_timeout);
                break;
            }
            Parsed::Request { id, method, params } => {
                if !authenticated {
                    let (auth_reply, ok) = authenticate(&String::from_utf8_lossy(&line), token);
                    if writeln!(out, "{auth_reply}").is_err() || !ok {
                        break;
                    }
                    authenticated = true;
                    continue;
                }
                // A repeated `auth` is answered here, never dispatched to the UI.
                if method == "auth" {
                    if writeln!(out, "{}", json!({"id": id, "ok": true, "result": {"authenticated": true}})).is_err() {
                        break;
                    }
                    continue;
                }
                // Ask Agent: answered here, on the connection's own thread.
                if let Some(mut r) = agents.handle(conn, &method, &params, || peer_open(&probe)) {
                    r["id"] = id;
                    ctx.request_repaint();
                    if writeln!(out, "{r}").is_err() {
                        break;
                    }
                    continue;
                }
                // The window checks Ask Agent ownership against this connection.
                let (mut req, rrx) = ControlRequest::new(method, params);
                req.agent_connection = Some(conn);
                let state = Arc::clone(&req.state);
                let mut r = match tx.try_send(req) {
                    Ok(()) => {
                        ctx.request_repaint();
                        wait_for_ui(&rrx, &state, &probe, agents, conn, limits)
                    }
                    Err(TrySendError::Full(_)) => json!({"ok": false, "error": "busy: too many queued requests"}),
                    Err(TrySendError::Disconnected(_)) => break,
                };
                if let Some(o) = r.as_object_mut() {
                    o.insert("id".into(), id);
                }
                r
            }
        };
        if let Err(e) = writeln!(out, "{reply}") {
            if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) {
                log::debug!("control client stopped reading replies; closing");
            }
            break;
        }
    }
    // Gone: it no longer counts as an agent, and a request it took without answering goes back in
    // the queue.
    agents.disconnect(conn);
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod auth_tests;
