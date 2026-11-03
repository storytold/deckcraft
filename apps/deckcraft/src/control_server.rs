//! Loopback JSON-lines control server: one request per line, one reply per line.
//! This is the transport the MCP server (`deckcraft-cli mcp --connect`) wraps.
//!
//! The server only reads requests: a line that is not a JSON object with a string `method` (an
//! HTTP request line, a stray `nc`, binary data) gets one error reply and the connection is
//! closed, so nothing sent after it runs. Every resource a client can hold is bounded
//! ([`Limits`]): simultaneous connections, request line length, queued requests, and how long a
//! socket may sit idle or blocked on a write.

use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::time::{Duration, Instant};

use deckcraft_ui_egui::ControlRequest;
use serde_json::{Value, json};

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

pub fn start(port: u16, ctx: egui::Context) -> Receiver<ControlRequest> {
    match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => {
            log::info!("control server listening on 127.0.0.1:{port}");
            serve_listener(l, Limits::default(), ctx)
        }
        Err(e) => {
            log::error!("control server failed to bind 127.0.0.1:{port}: {e}");
            sync_channel::<ControlRequest>(0).1
        }
    }
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
fn serve_listener(listener: TcpListener, limits: Limits, ctx: egui::Context) -> Receiver<ControlRequest> {
    let (tx, rx) = sync_channel::<ControlRequest>(limits.queue.max(1));
    let active = Arc::new(AtomicUsize::new(0));
    let refusing = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for mut stream in accepted(listener.incoming(), || std::thread::sleep(ACCEPT_RETRY)) {
            let _ = stream.set_write_timeout(Some(limits.write_timeout));
            if active.fetch_add(1, Ordering::SeqCst) >= limits.max_clients {
                active.fetch_sub(1, Ordering::SeqCst);
                let _ = writeln!(stream, "{}", json!({"ok": false, "error": "busy: too many control connections"}));
                refuse(stream, &refusing, limits.write_timeout.min(REFUSED_GRACE));
                continue;
            }
            let slot = Slot(Arc::clone(&active));
            let tx = tx.clone();
            let ctx = ctx.clone();
            let spawned = std::thread::Builder::new().name("control-client".into()).spawn(move || {
                let _slot = slot;
                serve(stream, &tx, &ctx, limits);
            });
            if spawned.is_err() {
                log::warn!("control server could not start a client thread");
            }
        }
    });
    rx
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

fn serve(stream: TcpStream, tx: &SyncSender<ControlRequest>, ctx: &egui::Context, limits: Limits) {
    if stream.set_read_timeout(Some(limits.idle_timeout)).is_err() || stream.set_write_timeout(Some(limits.write_timeout)).is_err() {
        return;
    }
    let Ok(read) = stream.try_clone() else { return };
    let mut out = stream;
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
                let (req, rrx) = ControlRequest::new(method, params);
                let state = Arc::clone(&req.state);
                let mut r = match tx.try_send(req) {
                    Ok(()) => {
                        ctx.request_repaint();
                        match rrx.recv_timeout(limits.reply_timeout) {
                            Ok(v) => v,
                            Err(_) => {
                                // Give up on the request. While it was still queued this makes
                                // sure it never runs late (#719); if the app had already taken
                                // it, its outcome is unknown rather than a definite failure.
                                if ControlRequest::abandon(&state) {
                                    json!({"ok": false, "error": "timeout"})
                                } else {
                                    json!({"ok": false, "error": "timeout: the app had already started the request, so it may still run; its outcome is unknown"})
                                }
                            }
                        }
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
}

#[cfg(test)]
mod tests;
