//! Where MCP tool calls end up: a control-channel method call.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde_json::{Value, json};

// Includes the newline, matching the control server's request budget. In particular a peer
// must not be able to grow a reply indefinitely by withholding its newline.
const MAX_REPLY_BYTES: u64 = 16 * 1024 * 1024;

/// Something that answers control-channel methods (`engine.execute`, `document.inspect`,
/// `ui.pointer`, `ui.render`, …). See `deckcraft_ui_egui::control` for the full list.
pub trait Backend {
    /// Call one method. `Ok` carries the `result`, `Err` the error message.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String>;
    /// True when a real UI is attached (`ui.screenshot`, `ui.click`, dialogs… work).
    fn has_ui(&self) -> bool;
    /// Short human description ("headless", "connected to 127.0.0.1:7979").
    fn describe(&self) -> String;
}

/// A running DeckCraft app, reached through its loopback control port.
pub struct Remote {
    addr: String,
    token: String,
    /// The token came from the app's own source (env or the token file), not from the caller,
    /// so a reconnect re-reads it: an app that restarted wrote a fresh token file, and the
    /// token from the first connection would authenticate nothing.
    file_backed: bool,
    conn: Option<(BufReader<TcpStream>, TcpStream)>,
    next_id: u64,
}

impl Remote {
    /// Connect to `addr` (`127.0.0.1:7979`), failing fast when nothing is listening. The app
    /// requires a token (#777): `DECKCRAFT_CONTROL_TOKEN`, else the file the app wrote for this port.
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        let token = crate::control_auth::client_token(addr).map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
        let mut r = Self { addr: addr.to_string(), token, file_backed: true, conn: None, next_id: 1 };
        r.reconnect()?;
        Ok(r)
    }

    /// Like [`Remote::connect`] with a token the caller already has (kept as given).
    pub fn connect_with_token(addr: &str, token: String) -> std::io::Result<Self> {
        let mut r = Self { addr: addr.to_string(), token, file_backed: false, conn: None, next_id: 1 };
        r.reconnect()?;
        Ok(r)
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }

    fn reconnect(&mut self) -> std::io::Result<()> {
        self.conn = None;
        // A file-backed token is re-read before authenticating: the app may have restarted
        // since the first connection and written a fresh token file. A lookup failure keeps
        // the stored token and lets the handshake decide. A caller-supplied token stays.
        if self.file_backed
            && let Ok(token) = crate::control_auth::client_token(&self.addr)
        {
            self.token = token;
        }
        let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, format!("cannot resolve {}", self.addr));
        for sa in self.addr.to_socket_addrs()? {
            match TcpStream::connect_timeout(&sa, Duration::from_millis(800)) {
                Ok(s) => {
                    s.set_nodelay(true).ok();
                    // The app answers within 60 s (its own timeout); leave headroom.
                    s.set_read_timeout(Some(Duration::from_secs(90))).ok();
                    let read = s.try_clone()?;
                    let mut conn = (BufReader::new(read), s);
                    Self::authenticate(&mut conn, &self.token)?;
                    self.conn = Some(conn);
                    return Ok(());
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// The handshake every connection starts with: the app closes one that doesn't send the token first.
    fn authenticate(conn: &mut (BufReader<TcpStream>, TcpStream), token: &str) -> std::io::Result<()> {
        let (reader, writer) = conn;
        writer.write_all(crate::control_auth::auth_line(0, token).as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        let mut reply = String::new();
        let n = std::io::Read::take(reader, 64 * 1024).read_line(&mut reply)?;
        let parsed = if n > 0 { serde_json::from_str::<Value>(reply.trim()).ok() } else { None };
        if parsed.as_ref().and_then(|v| v.get("ok")).and_then(Value::as_bool) == Some(true) {
            return Ok(());
        }
        // A `busy` refusal says the app is up and full, so retrying or freeing a slot is the fix —
        // the token message would send the user to check the wrong thing. Every other refusal keeps
        // the token hint, the app's own answer for a wrong or missing token.
        if let Some(error) = parsed.as_ref().and_then(|v| v.get("error")).and_then(Value::as_str)
            && error.starts_with("busy")
        {
            return Err(std::io::Error::new(std::io::ErrorKind::ConnectionRefused, error.to_string()));
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "the app refused the control token (is DECKCRAFT_CONTROL_TOKEN or the app's token file for this port out of date?)",
        ))
    }

    /// Write one request line. A failure here means the app never got a complete line, so retrying is safe.
    /// A connection the app has already closed (it restarted since the last call) is replaced before writing.
    fn send(&mut self, line: &str) -> std::io::Result<()> {
        if self.conn.as_ref().is_none_or(|(reader, writer)| !reader.buffer().is_empty() || is_closed(writer)) {
            self.reconnect()?;
        }
        let Some((_, writer)) = self.conn.as_mut() else {
            return Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "not connected"));
        };
        writer.write_all(format!("{line}\n").as_bytes())?;
        writer.flush()
    }

    /// Read the reply to a request that was already sent.
    fn receive(&mut self, id: u64) -> std::io::Result<Result<Value, String>> {
        let Some((reader, _)) = self.conn.as_mut() else {
            return Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "not connected"));
        };
        let mut reply = Vec::new();
        reader.take(MAX_REPLY_BYTES + 1).read_until(b'\n', &mut reply)?;
        if reply.len() as u64 > MAX_REPLY_BYTES {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "control reply exceeds 16 MiB"));
        }
        if reply.last() != Some(&b'\n') {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "control reply has no terminating newline"));
        }
        let invalid = |message| std::io::Error::new(std::io::ErrorKind::InvalidData, message);
        let v: Value = serde_json::from_slice(&reply).map_err(|e| invalid(format!("bad reply from app: {e}")))?;
        if v.get("id").and_then(Value::as_u64) != Some(id) {
            return Err(invalid("control reply has an unexpected request id".to_string()));
        }
        match v.get("ok").and_then(Value::as_bool) {
            Some(true) if v.get("error").is_none() => {
                v.get("result").cloned().map(Ok).ok_or_else(|| invalid("successful control reply has no result".to_string()))
            }
            Some(false) if v.get("result").is_none() => v
                .get("error")
                .and_then(Value::as_str)
                .map(|error| Err(error.to_string()))
                .ok_or_else(|| invalid("failed control reply has no error string".to_string())),
            _ => Err(invalid("control reply has an invalid envelope".to_string())),
        }
    }
}

/// True when the peer has closed or reset `stream`, or sent bytes nobody asked for. Between calls nothing
/// should be waiting, so only "would block" means the connection is still good.
fn is_closed(stream: &TcpStream) -> bool {
    if stream.set_nonblocking(true).is_err() {
        return true;
    }
    let mut byte = [0u8; 1];
    let open = matches!(stream.peek(&mut byte), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock);
    stream.set_nonblocking(false).is_err() || !open
}

impl Backend for Remote {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"id": id, "method": method, "params": params}).to_string();
        // One retry with a fresh connection (the app may have restarted), but only while the request
        // was not sent. Once it is out, a lost reply leaves the outcome unknown and a resend could
        // run an insert/delete/undo twice, so report that instead.
        if self.send(&line).is_err() {
            self.conn = None;
            self.send(&line).map_err(|e| {
                self.conn = None;
                format!("DeckCraft app at {} is not reachable: {e}", self.addr)
            })?;
        }
        self.receive(id).map_err(|e| {
            self.conn = None;
            format!(
                "lost the reply from the DeckCraft app at {} ({e}); the command may or may not have run, check the document before retrying",
                self.addr
            )
        })?
    }

    fn has_ui(&self) -> bool {
        true
    }

    fn describe(&self) -> String {
        format!("connected to the DeckCraft app at {}", self.addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const TEST_TOKEN: &str = "test-token-0123456789";

    /// The handshake a test peer answers before serving its scripted replies, as the app does.
    fn answer_auth(stream: &TcpStream) {
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).expect("auth line");
        let auth = serde_json::from_str::<Value>(&line).expect("auth JSON");
        assert_eq!(auth["method"], "auth");
        assert_eq!(auth["params"]["token"], TEST_TOKEN);
        let mut w = stream;
        w.write_all(b"{\"id\":0,\"ok\":true,\"result\":{\"authenticated\":true}}\n").expect("auth reply");
    }

    fn reply_once(reply: Vec<u8>) -> (Remote, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr").to_string();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            stream.set_read_timeout(Some(Duration::from_secs(5))).expect("timeout");
            stream.set_write_timeout(Some(Duration::from_secs(5))).expect("write timeout");
            answer_auth(&stream);
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).expect("request");
            assert_eq!(serde_json::from_str::<Value>(&line).expect("JSON")["id"], 1);
            // An oversized reply may be rejected before the peer finishes writing it.
            let _ = stream.write_all(&reply);
        });
        (Remote::connect_with_token(&addr, TEST_TOKEN.into()).expect("connect"), peer)
    }

    #[test]
    fn wrong_reply_id_is_not_command_success() {
        let (mut remote, peer) = reply_once(b"{\"id\":999,\"ok\":true,\"result\":\"unrelated\"}\n".to_vec());
        let result = remote.call("engine.execute", json!({"command":"slide.new"}));
        peer.join().expect("peer");
        let error = result.expect_err("an unrelated reply cannot confirm a mutation");
        assert!(error.contains("may or may not have run"), "{error}");
        assert!(remote.conn.is_none(), "discard the invalid connection without replaying");
    }

    /// A peer that refuses the handshake with a scripted error and closes.
    fn refuse_once(error: &'static str) -> std::io::Result<Remote> {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr").to_string();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            stream.set_read_timeout(Some(Duration::from_secs(5))).expect("timeout");
            stream.set_write_timeout(Some(Duration::from_secs(5))).expect("write timeout");
            // The auth line still arrives (the client sends it first); the refusal answers it.
            let _ = BufReader::new(&stream).read_line(&mut String::new());
            let _ = stream.write_all(format!("{{\"id\":0,\"ok\":false,\"error\":\"{error}\"}}\n").as_bytes());
        });
        let result = Remote::connect_with_token(&addr, TEST_TOKEN.into());
        peer.join().expect("peer");
        result
    }

    #[test]
    fn a_busy_refusal_reports_what_the_app_said_not_the_token() {
        let error = refuse_once("busy: too many control connections").err().expect("busy refuses");
        assert_eq!(error.kind(), std::io::ErrorKind::ConnectionRefused);
        assert!(error.to_string().starts_with("busy"), "{error}");
    }

    #[test]
    fn a_token_refusal_keeps_the_token_hint() {
        let error = refuse_once("authentication required").err().expect("the refusal refuses");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(error.to_string().contains("refused the control token"), "{error}");
    }

    /// A file-backed reconnect re-reads the token: the app restarted, wrote a fresh token
    /// file, and the token from the first connection authenticates nothing.
    #[test]
    fn a_file_backed_reconnect_follows_the_apps_new_token() {
        if std::env::var(crate::control_auth::TOKEN_ENV).is_ok_and(|t| !t.trim().is_empty()) {
            eprintln!("skipped: DECKCRAFT_CONTROL_TOKEN is set (it wins over the token file)");
            return;
        }
        // The listener takes its port first: a PID-derived port could belong to a running
        // app, whose token file write_token_file would replace before a bind failure.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let (token_a, token_b) = ("file-token-a-0123456789", "file-token-b-0123456789");
        let path = crate::control_auth::write_token_file(port, token_a).expect("write token file");
        let addr = format!("127.0.0.1:{port}");
        let peer = std::thread::spawn(move || {
            let check_auth = |stream: &TcpStream, expected: &str| {
                stream.set_read_timeout(Some(Duration::from_secs(5))).expect("timeout");
                stream.set_write_timeout(Some(Duration::from_secs(5))).expect("write timeout");
                let mut line = String::new();
                BufReader::new(stream).read_line(&mut line).expect("auth line");
                let auth = serde_json::from_str::<Value>(&line).expect("auth JSON");
                assert_eq!(auth["method"], "auth");
                assert_eq!(auth["params"]["token"], expected, "the reconnect sends the fresh token");
                let mut w = stream;
                w.write_all(b"{\"id\":0,\"ok\":true,\"result\":{\"authenticated\":true}}\n").expect("auth reply");
            };
            // First app run: accepts the token from the file.
            let (stream, _) = listener.accept().expect("accept 1");
            check_auth(&stream, token_a);
            drop(stream); // the app restarts
            // The restarted app: the fresh token authenticates.
            let (mut stream, _) = listener.accept().expect("accept 2");
            check_auth(&stream, token_b);
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).expect("request");
            stream.write_all(b"{\"id\":1,\"ok\":true,\"result\":null}\n").expect("request reply");
        });
        let mut remote = Remote::connect(&addr).expect("connect");
        std::fs::write(&path, format!("{token_b}\n")).expect("rewrite token file");
        let result = remote.call("engine.execute", json!({"command":"slide.new"}));
        peer.join().expect("peer");
        std::fs::remove_file(&path).ok();
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn matching_success_and_error_replies_keep_their_meaning() {
        for (reply, expected) in [
            ("{\"id\":1,\"ok\":true,\"result\":null}\r\n", Ok(Value::Null)),
            ("{\"id\":1,\"ok\":true,\"result\":{\"slides\":2}}\n", Ok(json!({"slides":2}))),
            ("{\"id\":1,\"ok\":false,\"error\":\"disabled\"}\n", Err("disabled".into())),
        ] {
            let (mut remote, peer) = reply_once(reply.as_bytes().to_vec());
            assert_eq!(remote.call("engine.execute", json!({})), expected);
            assert!(remote.conn.is_some(), "valid responses do not invalidate the connection");
            peer.join().expect("peer");
        }
    }

    #[test]
    fn malformed_reply_envelopes_leave_the_outcome_unknown() {
        for reply in [
            "not JSON\n",
            "[]\n",
            "{\"ok\":true,\"result\":null}\n",
            "{\"id\":\"1\",\"ok\":true,\"result\":null}\n",
            "{\"id\":1,\"ok\":1,\"result\":null}\n",
            "{\"id\":1,\"ok\":true}\n",
            "{\"id\":1,\"ok\":false}\n",
            "{\"id\":1,\"ok\":false,\"error\":42}\n",
            "{\"id\":1,\"ok\":true,\"result\":null,\"error\":\"failed\"}\n",
            "{\"id\":1,\"ok\":false,\"result\":null,\"error\":\"failed\"}\n",
        ] {
            let (mut remote, peer) = reply_once(reply.as_bytes().to_vec());
            let result = remote.call("engine.execute", json!({}));
            peer.join().expect("peer");
            let error = result.expect_err(reply);
            assert!(error.contains("may or may not have run"), "{reply}: {error}");
            assert!(remote.conn.is_none(), "{reply}: discard the invalid connection");
        }
    }

    #[test]
    fn eof_without_a_newline_is_not_a_complete_reply() {
        let (mut remote, peer) = reply_once(b"{\"id\":1,\"ok\":true,\"result\":null}".to_vec());
        let result = remote.call("engine.execute", json!({}));
        peer.join().expect("peer");
        assert!(result.is_err(), "a truncated frame cannot confirm a mutation");
        assert!(remote.conn.is_none());
    }

    #[test]
    fn oversized_reply_is_rejected() {
        let reply = format!("{{\"id\":1,\"ok\":true,\"result\":\"{}\"}}\n", "x".repeat(16 * 1024 * 1024));
        let (mut remote, peer) = reply_once(reply.into_bytes());
        let result = remote.call("engine.execute", json!({}));
        let invalidated = remote.conn.is_none();
        // Close the socket before joining: the bounded reader need not drain the peer's reply.
        remote.conn = None;
        peer.join().expect("peer");
        assert!(result.is_err(), "a reply is limited to 16 MiB");
        let error = result.expect_err("checked above");
        assert!(error.contains("may or may not have run"), "{error}");
        assert!(invalidated, "discard the oversized reply's connection");
    }

    #[test]
    fn reply_at_the_size_limit_is_accepted() {
        let prefix = "{\"id\":1,\"ok\":true,\"result\":\"";
        let suffix = "\"}\n";
        let payload_len = 16 * 1024 * 1024 - prefix.len() - suffix.len();
        let reply = format!("{prefix}{}{suffix}", "x".repeat(payload_len));
        let (mut remote, peer) = reply_once(reply.into_bytes());
        let result = remote.call("document.inspect", json!({})).expect("reply at the limit");
        assert_eq!(result.as_str().map(str::len), Some(payload_len));
        peer.join().expect("peer");
    }

    #[test]
    fn buffered_unsolicited_reply_is_discarded_before_sending() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr").to_string();
        let peer = std::thread::spawn(move || {
            let (mut first, _) = listener.accept().expect("first connection");
            first.set_read_timeout(Some(Duration::from_secs(5))).expect("timeout");
            answer_auth(&first);
            let mut line = String::new();
            BufReader::new(&first).read_line(&mut line).expect("initial request");
            // Even a stale frame whose ID happens to match must not answer a new request.
            first
                .write_all(b"{\"id\":1,\"ok\":true,\"result\":\"first\"}\n{\"id\":2,\"ok\":true,\"result\":\"stale\"}\n")
                .expect("two replies together");
            line.clear();
            let sent_on_stale = BufReader::new(&first).read_line(&mut line).expect("first request") != 0;
            if !sent_on_stale {
                let (mut fresh, _) = listener.accept().expect("fresh connection");
                fresh.set_read_timeout(Some(Duration::from_secs(5))).expect("timeout");
                answer_auth(&fresh);
                BufReader::new(&fresh).read_line(&mut line).expect("fresh request");
                fresh.write_all(b"{\"id\":2,\"ok\":false,\"error\":\"rejected\"}\n").expect("actual reply");
            }
            sent_on_stale
        });
        let mut remote = Remote::connect_with_token(&addr, TEST_TOKEN.into()).expect("connect");
        assert_eq!(remote.call("document.inspect", json!({})), Ok(json!("first")));
        // Force the stale frame into BufReader, where a socket-only peek cannot see it.
        assert!(!remote.conn.as_mut().expect("connection").0.fill_buf().expect("buffer").is_empty());
        let result = remote.call("engine.execute", json!({"command":"slide.new"}));
        let sent_on_stale = peer.join().expect("peer");
        assert_eq!(result, Err("rejected".into()), "only the fresh peer's response counts");
        assert!(!sent_on_stale, "replace a connection with buffered unsolicited bytes before sending");
    }

    #[test]
    fn lost_reply_is_not_resent() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr").to_string();
        let executed = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&executed);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                answer_auth(&stream);
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    continue;
                }
                let n = count.fetch_add(1, Ordering::SeqCst) + 1;
                if n > 1 {
                    let mut w = &stream;
                    let _ = w.write_all(b"{\"id\":2,\"ok\":true,\"result\":null}\n");
                }
                // The first request is executed, then the connection drops without a reply.
            }
        });
        let mut remote = Remote::connect_with_token(&addr, TEST_TOKEN.into()).expect("connect");
        let err = remote.call("engine.execute", json!({})).expect_err("lost reply must be an error");
        assert!(err.contains("may or may not have run"), "{err}");
        assert_eq!(executed.load(Ordering::SeqCst), 1, "the command must not be replayed");
        // The next call opens a fresh connection and works.
        assert!(remote.call("document.inspect", json!({})).is_ok());
        assert_eq!(executed.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn connection_closed_before_the_call_is_retried() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr").to_string();
        let (closed_tx, closed_rx) = std::sync::mpsc::channel();
        let executed = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&executed);
        std::thread::spawn(move || {
            let mut first = true;
            for stream in listener.incoming().flatten() {
                if first {
                    // The app restarts: the first connection completes the handshake, then closes
                    // before any request arrives.
                    first = false;
                    answer_auth(&stream);
                    drop(stream);
                    let _ = closed_tx.send(());
                    continue;
                }
                answer_auth(&stream);
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    continue;
                }
                count.fetch_add(1, Ordering::SeqCst);
                let mut w = &stream;
                let _ = w.write_all(b"{\"id\":1,\"ok\":true,\"result\":null}\n");
            }
        });
        let mut remote = Remote::connect_with_token(&addr, TEST_TOKEN.into()).expect("connect");
        closed_rx.recv().expect("server closed the first connection");
        std::thread::sleep(Duration::from_millis(200));
        assert!(remote.call("engine.execute", json!({})).is_ok(), "a request that never left must be sent on a fresh connection");
        assert_eq!(executed.load(Ordering::SeqCst), 1);
    }
}
