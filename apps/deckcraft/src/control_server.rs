//! Loopback JSON-lines control server: one request per line, one reply per line.
//! This is the transport the MCP server (`deckcraft-cli mcp --connect`) wraps.
//!
//! The server only reads requests: a line that is not a JSON object with a string `method` (an
//! HTTP request line, a stray `nc`, binary data) gets one error reply and the connection is
//! closed, so nothing sent after it runs. Every resource a client can hold is bounded
//! ([`Limits`]): simultaneous connections, request line length, queued requests, and how long a
//! socket may sit idle or blocked on a write.

use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::time::Duration;

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

/// Accept connections on `listener` under `limits`; requests arrive on the returned receiver.
fn serve_listener(listener: TcpListener, limits: Limits, ctx: egui::Context) -> Receiver<ControlRequest> {
    let (tx, rx) = sync_channel::<ControlRequest>(limits.queue.max(1));
    let active = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_write_timeout(Some(limits.write_timeout));
            if active.fetch_add(1, Ordering::SeqCst) >= limits.max_clients {
                active.fetch_sub(1, Ordering::SeqCst);
                let _ = writeln!(stream, "{}", json!({"ok": false, "error": "busy: too many control connections"}));
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
                break;
            }
            Line::Closed => break,
        };
        let reply = match parse(&line) {
            Parsed::Blank => continue,
            Parsed::Reject(e) => {
                // The connection closes either way; a peer that can't read the reason loses nothing.
                let _ = writeln!(out, "{}", json!({"ok": false, "error": e}));
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
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::time::Instant;

    fn small() -> Limits {
        Limits {
            max_clients: 2,
            max_line: 64,
            queue: 1,
            idle_timeout: Duration::from_millis(300),
            write_timeout: Duration::from_millis(300),
            reply_timeout: Duration::from_millis(300),
        }
    }

    fn server(limits: Limits) -> (SocketAddr, Receiver<ControlRequest>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        (addr, serve_listener(l, limits, egui::Context::default()))
    }

    fn client(addr: SocketAddr) -> (BufReader<TcpStream>, TcpStream) {
        let s = TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        (BufReader::new(s.try_clone().unwrap()), s)
    }

    fn reply(r: &mut BufReader<TcpStream>) -> Option<Value> {
        let mut s = String::new();
        match r.read_line(&mut s) {
            Ok(0) | Err(_) => None,
            Ok(_) => serde_json::from_str(&s).ok(),
        }
    }

    /// Answer every request with its method name until the server goes away.
    fn ui(rx: Receiver<ControlRequest>) {
        std::thread::spawn(move || {
            for req in rx {
                let _ = req.reply.send(json!({"ok": true, "result": req.method}));
            }
        });
    }

    #[test]
    fn request_round_trip() {
        let (addr, rx) = server(small());
        ui(rx);
        let (mut r, mut w) = client(addr);
        writeln!(w, r#"{{"id":7,"method":"ui.inspect"}}"#).unwrap();
        let v = reply(&mut r).unwrap();
        assert_eq!(v["id"], 7);
        assert_eq!(v["result"], "ui.inspect");
    }

    /// Every reply line until the server closes the connection.
    fn replies_until_closed(r: &mut BufReader<TcpStream>) -> Vec<Value> {
        std::iter::from_fn(|| reply(r)).collect()
    }

    /// A browser can POST to the port: its request line closes the connection before the JSON
    /// line in its body is read, so nothing is dispatched.
    #[test]
    fn an_http_request_closes_the_connection_before_its_body_is_dispatched() {
        let (addr, rx) = server(Limits { max_line: 512, ..small() });
        let (mut r, mut w) = client(addr);
        w.write_all(
            b"POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: text/plain\r\n\r\n\
              {\"id\":1,\"method\":\"engine.execute\",\"params\":{\"command\":\"slide.new\"}}\n",
        )
        .unwrap();
        let replies = replies_until_closed(&mut r);
        assert_eq!(replies.len(), 1, "{replies:?}");
        assert!(replies[0]["error"].as_str().unwrap().contains("closing the connection"), "{replies:?}");
        assert!(rx.try_recv().is_err(), "the body was dispatched");
    }

    #[test]
    fn a_line_that_is_not_a_request_closes_the_connection() {
        let (addr, rx) = server(small());
        ui(rx);
        for junk in [&b"hello\n"[..], b"[1,2]\n", b"{\"id\":1}\n", b"{\"method\":5}\n", b"\xff\xfe\n", b"GET /x HTTP/1.1\n"] {
            let (mut r, mut w) = client(addr);
            w.write_all(junk).unwrap();
            w.write_all(b"{\"id\":2,\"method\":\"m\"}\n").unwrap();
            let replies = replies_until_closed(&mut r);
            assert_eq!(replies.len(), 1, "{replies:?}");
            assert_eq!(replies[0]["ok"], false);
        }
        assert!(is_http_request_line("OPTIONS * HTTP/1.1"));
        assert!(is_http_request_line("PRI * HTTP/2.0"));
        assert!(!is_http_request_line("{\"method\":\"GET /\"}"));
    }

    #[test]
    fn oversized_line_closes_connection() {
        let (addr, rx) = server(small());
        ui(rx);
        let (mut r, mut w) = client(addr);
        w.write_all(&[b'x'; 200]).unwrap();
        let v = reply(&mut r).unwrap();
        assert!(v["error"].as_str().unwrap().contains("longer than 64"));
        assert!(reply(&mut r).is_none());
    }

    #[test]
    fn idle_connection_is_closed() {
        let (addr, rx) = server(small());
        ui(rx);
        let (mut r, mut w) = client(addr);
        w.write_all(b"{\"id\":1,").unwrap(); // unterminated, then silence
        let t = Instant::now();
        assert!(reply(&mut r).is_none());
        assert!(t.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn connection_limit_refuses_then_frees_slot() {
        let (addr, rx) = server(small());
        ui(rx);
        let a = client(addr);
        let (mut br, mut bw) = client(addr);
        // Make sure both are being served before the third arrives.
        writeln!(bw, r#"{{"id":1,"method":"m"}}"#).unwrap();
        assert!(reply(&mut br).is_some());
        let (mut cr, _cw) = client(addr);
        let v = reply(&mut cr).unwrap();
        assert!(v["error"].as_str().unwrap().starts_with("busy"));
        drop(a);
        // The freed slot is reusable once the server notices the disconnect.
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            let (mut dr, mut dw) = client(addr);
            writeln!(dw, r#"{{"id":2,"method":"m"}}"#).unwrap();
            if reply(&mut dr).is_some_and(|v| v["ok"] == true) {
                break;
            }
            assert!(Instant::now() < deadline, "slot never freed");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn blocked_ui_times_out_and_full_queue_is_busy() {
        let (addr, rx) = server(small());
        // No UI: the first request fills the one-slot queue and times out; the second finds it full.
        let (mut ar, mut aw) = client(addr);
        let (mut br, mut bw) = client(addr);
        writeln!(aw, r#"{{"id":1,"method":"m"}}"#).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        writeln!(bw, r#"{{"id":2,"method":"m"}}"#).unwrap();
        let b = reply(&mut br).unwrap();
        assert!(b["error"].as_str().unwrap().starts_with("busy"));
        assert_eq!(b["id"], 2);
        let a = reply(&mut ar).unwrap();
        assert_eq!(a["error"], "timeout");
        drop(rx);
    }

    /// A request that timed out while it was still queued must not run when the window resumes (#719).
    #[test]
    fn timed_out_request_does_not_run_when_the_window_resumes() {
        let (addr, rx) = server(Limits { max_line: 512, ..small() });
        let (mut r, mut w) = client(addr);
        writeln!(w, r#"{{"id":1,"method":"engine.execute","params":{{"command":"slide.new"}}}}"#).unwrap();
        let t = reply(&mut r).unwrap();
        assert_eq!(t["error"], "timeout");
        // The window resumes and drains the queue, exactly as `SlideApp::drain_control` does.
        let mut app = deckcraft_ui_egui::SlideApp::new(deckcraft_engine::Session::with_new(), deckcraft_ui_egui::Services::default());
        let ctx = egui::Context::default();
        let before = app.session.active().unwrap().doc.slides.len();
        let req = rx.try_recv().expect("the timed-out request is still queued");
        if req.take() {
            let _ = deckcraft_ui_egui::control::handle(&mut app, &ctx, &req);
        }
        assert_eq!(app.session.active().unwrap().doc.slides.len(), before, "a request that timed out must not run when the window resumes");
        drop(rx);
    }

    /// A request the app had already started when the reply timed out reports an unknown outcome,
    /// not a definite failure (#719).
    #[test]
    fn a_request_already_started_when_the_reply_times_out_reports_an_unknown_outcome() {
        let (addr, rx) = server(small());
        let ui = std::thread::spawn(move || {
            let req = rx.recv().expect("the request is queued");
            assert!(req.take(), "the app claims the request");
            // Still running it when the sender's wait expires.
            std::thread::sleep(Duration::from_millis(450));
            let _ = req.reply.send(json!({"ok": true, "result": "ran"}));
        });
        let (mut r, mut w) = client(addr);
        writeln!(w, r#"{{"id":1,"method":"m"}}"#).unwrap();
        let v = reply(&mut r).unwrap();
        assert!(v["error"].as_str().unwrap().contains("outcome is unknown"), "{v}");
        ui.join().unwrap();
    }
}
