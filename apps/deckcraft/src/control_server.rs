//! Loopback JSON-lines control server: one request per line, one reply per line.
//! This is the transport the MCP server (`deckcraft-cli mcp --connect`) wraps.
//!
//! Every resource a client can hold is bounded ([`Limits`]): simultaneous connections, request
//! line length, queued requests, and how long a socket may sit idle or blocked on a write.
//!
//! A connection has to present the app's token before it may send anything else: `127.0.0.1` keeps
//! the channel off the network, but any process on this machine can reach it, and an
//! authenticated method can quit the app or read and write files at a path of the caller's
//! choosing. The handshake ([`deckcraft_engine::control_auth::HELLO`]) is answered by this
//! transport, not by the UI, so an unauthenticated peer cannot reach the request queue either.

use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::time::Duration;

use deckcraft_engine::control_auth::{self, HELLO};
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
            // Only an authenticated client reaches this, which is what keeps the bound
            // generous — see `docs/control-protocol.md`.
            max_line: 256 * 1024 * 1024,
            queue: 64,
            idle_timeout: Duration::from_secs(600),
            write_timeout: Duration::from_secs(30),
            reply_timeout: Duration::from_secs(60),
        }
    }
}

pub fn start(port: u16, token: &str, ctx: egui::Context) -> Receiver<ControlRequest> {
    match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => {
            log::info!("control server listening on 127.0.0.1:{port}");
            serve_listener(l, Limits::default(), token, ctx)
        }
        Err(e) => {
            log::error!("control server failed to bind 127.0.0.1:{port}: {e}");
            sync_channel::<ControlRequest>(0).1
        }
    }
}

/// Accept connections on `listener` under `limits`; requests arrive on the returned receiver.
fn serve_listener(listener: TcpListener, limits: Limits, token: &str, ctx: egui::Context) -> Receiver<ControlRequest> {
    let (tx, rx) = sync_channel::<ControlRequest>(limits.queue.max(1));
    let active = Arc::new(AtomicUsize::new(0));
    // The client threads outlive this call, so the token they compare against has to be owned —
    // and shared, since each connection's thread gets its own handle.
    let token = Arc::new(token.to_string());
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
            let token = Arc::clone(&token);
            let spawned = std::thread::Builder::new().name("control-client".into()).spawn(move || {
                let _slot = slot;
                serve(stream, &tx, &ctx, limits, &token);
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

fn serve(stream: TcpStream, tx: &SyncSender<ControlRequest>, ctx: &egui::Context, limits: Limits, token: &str) {
    if stream.set_read_timeout(Some(limits.idle_timeout)).is_err() || stream.set_write_timeout(Some(limits.write_timeout)).is_err() {
        return;
    }
    let Ok(read) = stream.try_clone() else { return };
    let mut out = stream;
    let mut reader = BufReader::new(read);
    // Nothing but the handshake may arrive before this is true.
    let mut authenticated = false;
    loop {
        let line = match read_line(&mut reader, limits.max_line) {
            Line::Text(t) => t,
            Line::TooLong => {
                let _ = writeln!(out, "{}", json!({"ok": false, "error": format!("request line longer than {} bytes", limits.max_line)}));
                break;
            }
            Line::Closed => break,
        };
        let Ok(line) = String::from_utf8(line) else {
            if writeln!(out, "{}", json!({"ok": false, "error": "bad JSON: not UTF-8"})).is_err() {
                break;
            }
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }
        // Set when this request ends the connection, which a refused handshake does: the peer has
        // to start over with the token rather than keep guessing on the same socket.
        let mut close = false;
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => {
                let req_id = msg.get("id").cloned().unwrap_or(Value::Null);
                let method = msg.get("method").and_then(Value::as_str).unwrap_or("").to_string();
                let params = msg.get("params").cloned().unwrap_or(json!({}));

                if !authenticated {
                    // Answered here rather than queued, so an unauthenticated peer can neither
                    // run a method nor keep the app's request queue full.
                    let given = params.get("token").and_then(Value::as_str).unwrap_or("");
                    let mut r = if method == HELLO && control_auth::matches(token, given) {
                        authenticated = true;
                        json!({"ok": true, "result": HELLO})
                    } else {
                        // Neither the reply nor the log may repeat the token that was offered.
                        close = true;
                        if method == HELLO {
                            log::warn!("control server refused a handshake: wrong token");
                            json!({"ok": false, "error": "unauthorized: wrong control token"})
                        } else {
                            json!({"ok": false, "error": format!("unauthorized: send {HELLO} with the control token first")})
                        }
                    };
                    if let Some(o) = r.as_object_mut() {
                        o.insert("id".into(), req_id);
                    }
                    r
                } else {
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
                        o.insert("id".into(), req_id);
                    }
                    r
                }
            }
            Err(e) => json!({"ok": false, "error": format!("bad JSON: {e}")}),
        };
        if let Err(e) = writeln!(out, "{reply}") {
            if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) {
                log::debug!("control client stopped reading replies; closing");
            }
            break;
        }
        if close {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::time::Instant;

    /// The token the test server expects; any other token must be refused.
    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn small() -> Limits {
        Limits {
            max_clients: 2,
            // Room for the handshake line, which carries a token. The oversize test writes past it.
            max_line: 512,
            queue: 1,
            idle_timeout: Duration::from_millis(300),
            write_timeout: Duration::from_millis(300),
            reply_timeout: Duration::from_millis(300),
        }
    }

    fn server(limits: Limits) -> (SocketAddr, Receiver<ControlRequest>) {
        server_with_token(limits, TOKEN)
    }

    fn server_with_token(limits: Limits, token: &str) -> (SocketAddr, Receiver<ControlRequest>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        (addr, serve_listener(l, limits, token, egui::Context::default()))
    }

    fn client(addr: SocketAddr) -> (BufReader<TcpStream>, TcpStream) {
        let s = TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        (BufReader::new(s.try_clone().unwrap()), s)
    }

    /// Send the handshake and return the connection once the server has accepted it.
    fn authed(addr: SocketAddr) -> (BufReader<TcpStream>, TcpStream) {
        let (mut r, mut w) = client(addr);
        hello(&mut w, TOKEN);
        assert!(reply(&mut r).is_some_and(|v| v["ok"] == true), "the handshake must be accepted");
        (r, w)
    }

    fn hello(w: &mut impl Write, token: &str) {
        writeln!(w, r#"{{"id":0,"method":"{HELLO}","params":{{"token":"{token}"}}}}"#).unwrap();
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
        let (mut r, mut w) = authed(addr);
        writeln!(w, r#"{{"id":7,"method":"ui.inspect"}}"#).unwrap();
        let v = reply(&mut r).unwrap();
        assert_eq!(v["id"], 7);
        assert_eq!(v["result"], "ui.inspect");
    }

    /// A request before the handshake is refused and the connection ends, so a peer on this
    /// machine cannot drive the app it did not authenticate to.
    #[test]
    fn a_request_before_the_handshake_is_refused() {
        let (addr, rx) = server(small());
        ui(rx);
        let (mut r, mut w) = client(addr);
        writeln!(w, r#"{{"id":1,"method":"app.quit"}}"#).unwrap();
        let v = reply(&mut r).unwrap();
        assert!(v["error"].as_str().unwrap().contains("unauthorized"), "{v}");
        assert_eq!(v["id"], 1);
        assert!(reply(&mut r).is_none(), "a refused handshake closes the connection");
    }

    #[test]
    fn a_wrong_token_is_refused_and_never_echoed() {
        let (addr, rx) = server_with_token(small(), TOKEN);
        ui(rx);
        let wrong = "ffffffffffffffffffffffffffffffff";
        let (mut r, mut w) = client(addr);
        hello(&mut w, wrong);
        let v = reply(&mut r).unwrap();
        assert!(v["error"].as_str().unwrap().contains("wrong control token"), "{v}");
        assert!(!v.to_string().contains(wrong), "the offered token must not come back: {v}");
        assert!(reply(&mut r).is_none());
    }

    /// A missing or empty token is as wrong as a wrong one.
    #[test]
    fn a_handshake_without_a_token_is_refused() {
        // No UI thread: the transport answers a refused handshake without one.
        let (addr, _rx) = server(small());
        let (mut r, mut w) = client(addr);
        writeln!(w, r#"{{"id":1,"method":"{HELLO}","params":{{}}}}"#).unwrap();
        assert!(reply(&mut r).unwrap()["error"].as_str().unwrap().contains("unauthorized"));
    }

    /// The handshake is answered by the transport, so it costs the app's request queue nothing —
    /// which is what stops an unauthenticated peer from filling it.
    #[test]
    fn the_handshake_does_not_reach_the_ui_queue() {
        let (addr, rx) = server(small());
        // No UI thread: a queued request would time out rather than be answered.
        let (mut r, mut w) = client(addr);
        hello(&mut w, TOKEN);
        assert_eq!(reply(&mut r).unwrap()["result"], HELLO);
        assert!(rx.try_recv().is_err(), "the handshake must not be queued for the UI");
    }

    #[test]
    fn oversized_line_closes_connection() {
        let (addr, rx) = server(small());
        ui(rx);
        let (mut r, mut w) = authed(addr);
        w.write_all(&[b'x'; 2000]).unwrap();
        let v = reply(&mut r).unwrap();
        assert!(v["error"].as_str().unwrap().contains("longer than 512"));
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
        let (mut br, mut bw) = authed(addr);
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
            let (mut dr, mut dw) = authed(addr);
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
        let (mut ar, mut aw) = authed(addr);
        let (mut br, mut bw) = authed(addr);
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
        let (mut r, mut w) = authed(addr);
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
        let (mut r, mut w) = authed(addr);
        writeln!(w, r#"{{"id":1,"method":"m"}}"#).unwrap();
        let v = reply(&mut r).unwrap();
        assert!(v["error"].as_str().unwrap().contains("outcome is unknown"), "{v}");
        ui.join().unwrap();
    }
}
