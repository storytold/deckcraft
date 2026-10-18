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
use std::net::{TcpListener, TcpStream};
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

/// Accept connections on `listener` under `limits`; requests arrive on the returned receiver.
fn serve_on(listener: TcpListener, token: String, limits: Limits, ctx: egui::Context, agents: AgentHub) -> Receiver<ControlRequest> {
    let (tx, rx) = sync_channel::<ControlRequest>(limits.queue.max(1));
    let token = Arc::new(token);
    let active = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        // Each connection's number, for Ask Agent's presence.
        let mut conn = 0u64;
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_write_timeout(Some(limits.write_timeout));
            if active.fetch_add(1, Ordering::SeqCst) >= limits.max_clients {
                active.fetch_sub(1, Ordering::SeqCst);
                let _ = writeln!(stream, "{}", json!({"ok": false, "error": "busy: too many control connections"}));
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
                if !authenticated {
                    let is_auth = method == "auth";
                    let given = params.get("token").and_then(Value::as_str);
                    let (auth_reply, ok) = match given {
                        Some(t) if is_auth && auth::tokens_match(t, token) => {
                            (json!({"id": id, "ok": true, "result": {"authenticated": true}}), true)
                        }
                        _ if is_auth => (json!({"id": id, "ok": false, "error": "wrong token"}), false),
                        _ => (json!({"id": id, "ok": false, "error": AUTH_FIRST}), false),
                    };
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
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef";

    #[test]
    fn only_the_right_token_authenticates() {
        let (reply, ok) = authenticate(&json!({"id": 1, "method": "auth", "params": {"token": TOKEN}}).to_string(), TOKEN);
        assert!(ok);
        assert_eq!(reply["ok"], true);
        assert_eq!(reply["id"], 1);
        for line in [
            json!({"id": 1, "method": "auth", "params": {"token": "wrong"}}).to_string(),
            json!({"id": 1, "method": "auth", "params": {}}).to_string(),
            json!({"id": 1, "method": "auth"}).to_string(),
            json!({"id": 1, "method": "app.save", "params": {"path": "/tmp/x.pptx", "token": TOKEN}}).to_string(),
        ] {
            let (reply, ok) = authenticate(&line, TOKEN);
            assert!(!ok, "{line}");
            assert_eq!(reply["ok"], false, "{line}");
        }
    }

    #[test]
    fn an_http_request_is_refused() {
        // The first line a browser sends for `fetch("http://127.0.0.1:PORT", {method: "POST", body})`.
        let (reply, ok) = authenticate("POST / HTTP/1.1", TOKEN);
        assert!(!ok);
        assert_eq!(reply["ok"], false);
    }

    /// Over a real socket: an unauthenticated request is refused and the connection closed; after
    /// `auth`, requests reach the UI channel.
    #[test]
    fn requests_reach_the_ui_only_after_auth() {
        use std::io::Read as _;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = sync_channel(4);
        let server = std::thread::spawn(move || {
            for stream in listener.incoming().take(2) {
                serve(stream.unwrap(), TOKEN, &tx, &egui::Context::default(), &AgentHub::default(), 1, Limits::default());
            }
        });

        // No auth: refused, closed, nothing dispatched.
        let mut s = TcpStream::connect(addr).unwrap();
        writeln!(s, "{}", json!({"id": 1, "method": "app.quit"})).unwrap();
        let mut rest = String::new();
        s.read_to_string(&mut rest).unwrap();
        assert!(rest.contains("authenticate first"), "{rest}");
        assert!(rx.try_recv().is_err());

        // Auth, then a request the UI answers.
        let s = TcpStream::connect(addr).unwrap();
        let mut w = s.try_clone().unwrap();
        let mut r = BufReader::new(s);
        writeln!(w, "{}", json!({"id": 1, "method": "auth", "params": {"token": TOKEN}})).unwrap();
        let mut line = String::new();
        r.read_line(&mut line).unwrap();
        assert!(line.contains("\"authenticated\":true"), "{line}");
        writeln!(w, "{}", json!({"id": 2, "method": "ui.inspect"})).unwrap();
        let req = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(req.method, "ui.inspect");
        let _ = req.reply.send(json!({"ok": true, "result": {}}));
        line.clear();
        r.read_line(&mut line).unwrap();
        assert!(line.contains("\"id\":2"), "{line}");
        drop((w, r));
        server.join().unwrap();
    }

    // Ask Agent.

    use deckcraft_ui_egui::ask_agent::{Request, Status};
    use std::time::Instant;

    fn boot() -> (u16, Receiver<ControlRequest>, AgentHub) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let agents = AgentHub::default();
        (port, serve_on(listener, TOKEN.to_string(), Limits::default(), egui::Context::default(), agents.clone()), agents)
    }

    struct Conn(BufReader<TcpStream>, TcpStream);

    impl Conn {
        fn authed(port: u16) -> Self {
            let s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
            let mut c = Conn(BufReader::new(s.try_clone().unwrap()), s);
            assert_eq!(c.call(&json!({"id": 0, "method": "auth", "params": {"token": TOKEN}}))["ok"], true);
            c
        }
        fn call(&mut self, v: &Value) -> Value {
            writeln!(self.1, "{v}").unwrap();
            let mut line = String::new();
            self.0.read_line(&mut line).unwrap();
            serde_json::from_str(&line).unwrap_or(Value::Null)
        }
    }

    fn ask(agents: &AgentHub) -> u64 {
        let selection = json!({"kind": "shapes", "shapes": [{"id": 258, "text": "Kilns fire clay."}]});
        agents.ask(Request { id: 0, instruction: "Tighten".into(), history: vec![], selection, document: json!({}), agent: None })
    }

    #[test]
    fn an_agent_listens_and_replies_over_the_socket() {
        let (port, _rx, agents) = boot();
        let id = ask(&agents);
        let mut c = Conn::authed(port);
        let got = c.call(&json!({"id": 1, "method": "agent.listen", "params": {"agent": "Claude", "timeoutMs": 1000}}));
        assert_eq!((got["id"].clone(), got["result"]["id"].clone()), (json!(1), json!(id)));
        assert_eq!(c.call(&json!({"id": 2, "method": "agent.reply", "params": {"id": id, "text": "Tightened."}}))["ok"], true);
        assert_eq!(agents.snapshot(id).unwrap().status, Status::Answered);
    }

    #[test]
    fn a_client_that_hangs_up_gives_its_request_back() {
        let (port, _rx, agents) = boot();
        let id = ask(&agents);
        let mut c = Conn::authed(port);
        assert_eq!(c.call(&json!({"id": 1, "method": "agent.listen", "params": {"timeoutMs": 1000}}))["result"]["id"], id);
        drop(c);
        let t0 = Instant::now();
        while agents.snapshot(id).unwrap().status != Status::Queued && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(agents.snapshot(id).unwrap().status, Status::Queued, "back in the queue for the next agent");
    }
}

#[cfg(test)]
mod limits_tests {
    use super::*;
    use std::net::SocketAddr;
    use std::time::Instant;

    fn small() -> Limits {
        Limits {
            max_clients: 2,
            max_line: 128,
            queue: 1,
            idle_timeout: Duration::from_millis(300),
            write_timeout: Duration::from_millis(300),
            reply_timeout: Duration::from_millis(300),
        }
    }

    fn server(limits: Limits) -> (SocketAddr, Receiver<ControlRequest>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        (addr, serve_on(l, TOKEN.into(), limits, egui::Context::default(), AgentHub::default()))
    }

    const TOKEN: &str = "0123456789abcdef";

    fn raw(addr: SocketAddr) -> (BufReader<TcpStream>, TcpStream) {
        let s = TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        (BufReader::new(s.try_clone().unwrap()), s)
    }

    /// An authenticated connection.
    fn client(addr: SocketAddr) -> (BufReader<TcpStream>, TcpStream) {
        let (mut r, mut w) = raw(addr);
        writeln!(w, "{}", json!({"id": 0, "method": "auth", "params": {"token": TOKEN}})).unwrap();
        assert_eq!(reply(&mut r).unwrap()["ok"], true);
        (r, w)
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
            let (mut r, mut w) = raw(addr);
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
        w.write_all(&[b'x'; 300]).unwrap();
        let v = reply(&mut r).unwrap();
        assert!(v["error"].as_str().unwrap().contains("longer than 128"));
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
        let (mut cr, _cw) = raw(addr);
        let v = reply(&mut cr).unwrap();
        assert!(v["error"].as_str().unwrap().starts_with("busy"));
        drop(a);
        // The freed slot is reusable once the server notices the disconnect.
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            let (mut dr, mut dw) = raw(addr);
            // A connection refused as busy is closed at once: the write may fail; try again.
            let _ = writeln!(dw, "{}", json!({"id": 0, "method": "auth", "params": {"token": TOKEN}}));
            if reply(&mut dr).is_some_and(|v| v["ok"] == true) {
                let _ = writeln!(dw, r#"{{"id":2,"method":"m"}}"#);
                if reply(&mut dr).is_some_and(|v| v["ok"] == true) {
                    break;
                }
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
