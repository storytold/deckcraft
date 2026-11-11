use super::*;

/// A failed accept is followed by a wait before the next one, so a listener that keeps
/// failing does not spin, and the connections around the failures still come through.
#[test]
fn a_failed_accept_waits_before_the_next() {
    let failed = || Err(std::io::Error::other("no file descriptors left"));
    let incoming = vec![failed(), Ok(1), failed(), failed(), Ok(2)];
    let mut waits = 0;
    let connections: Vec<i32> = super::accepted(incoming.into_iter(), || waits += 1).collect();
    assert_eq!(connections, [1, 2]);
    assert_eq!(waits, 3, "one wait per failed accept");
}
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

/// A peer that pipelines -- two lines in one write, as a script does -- still reads why its
/// line was rejected. Closing with its second line unread resets the connection and discards
/// the reply already on its way, so the peer saw nothing at all; repeated here because it
/// depended on the timing of the two.
#[test]
fn a_pipelined_peer_still_reads_why_its_line_was_rejected() {
    // A slot comes back only when the thread serving a peer ends, which can be after the next
    // peer connects, so every peer has a slot of its own.
    const PEERS: usize = 12;
    let (addr, rx) = server(Limits { max_clients: PEERS, ..small() });
    for _ in 0..PEERS {
        let (mut r, mut w) = client(addr);
        w.write_all(b"hello\n").unwrap();
        // A second line, written after the first as a script writes it: it races the close.
        let _ = w.write_all(b"{\"id\":2,\"method\":\"m\"}\n");
        let replies = replies_until_closed(&mut r);
        assert_eq!(replies.len(), 1, "{replies:?}");
        assert_eq!(replies[0]["ok"], false);
        assert!(replies[0]["error"].as_str().unwrap_or_default().contains("closing the connection"), "{replies:?}");
    }
    // The line after the rejected one is never dispatched, pipelined or not.
    assert!(rx.try_recv().is_err(), "a line after the rejection was dispatched");
}

/// A peer refused because every connection is taken still reads why when it had already
/// written a line: closing with that line unread resets the connection and discards the
/// reply. Repeated because it depends on the timing of the two.
#[test]
fn a_pipelined_peer_refused_as_busy_still_reads_why() {
    let (addr, rx) = server(Limits { max_clients: 1, idle_timeout: Duration::from_secs(30), ..small() });
    ui(rx);
    // Take the only slot, and make sure it is being served.
    let (mut hr, mut hw) = client(addr);
    writeln!(hw, r#"{{"id":1,"method":"m"}}"#).unwrap();
    assert!(reply(&mut hr).is_some());
    for _ in 0..50 {
        let (mut r, mut w) = client(addr);
        let _ = w.write_all(b"{\"id\":2,\"method\":\"m\"}\n");
        let replies = replies_until_closed(&mut r);
        assert_eq!(replies.len(), 1, "{replies:?}");
        assert!(replies[0]["error"].as_str().unwrap_or_default().starts_with("busy"), "{replies:?}");
    }
}

/// Peers refused as busy that stay connected without sending anything do not hold up the
/// connections after them: each one's drain runs to its deadline on a thread of its own, and
/// past `MAX_REFUSING` drains at once a refused connection is closed without one.
#[test]
fn silent_refused_peers_do_not_delay_the_next_refusal() {
    let (addr, rx) = server(Limits { max_clients: 1, idle_timeout: Duration::from_secs(30), ..small() });
    ui(rx);
    let (mut hr, mut hw) = client(addr);
    writeln!(hw, r#"{{"id":1,"method":"m"}}"#).unwrap();
    assert!(reply(&mut hr).is_some());
    // Twenty drains of 250 ms each, one after another, would take five seconds.
    let silent: Vec<_> = (0..MAX_REFUSING + 4).map(|_| client(addr)).collect();
    let start = Instant::now();
    let (mut r, _w) = client(addr);
    let refused = reply(&mut r).unwrap();
    assert!(refused["error"].as_str().unwrap_or_default().starts_with("busy"), "{refused}");
    assert!(start.elapsed() < Duration::from_secs(1), "refused after {:?}", start.elapsed());
    drop(silent);
}

/// A peer that keeps its connection open after its line was rejected gives its slot back at
/// the drain deadline (the write timeout), not at the idle timeout.
#[test]
fn a_rejected_peer_that_stays_connected_frees_its_slot_at_the_deadline() {
    let (addr, rx) = server(Limits { max_clients: 1, idle_timeout: Duration::from_secs(30), ..small() });
    ui(rx);
    let (mut r, mut w) = client(addr);
    w.write_all(b"hello\n").unwrap();
    assert_eq!(reply(&mut r).unwrap()["ok"], false);
    // `w` stays open, sending nothing.
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
    drop(w);
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
    // `a` sends nothing. Under a 300 ms idle timeout, a slow run could close it before the
    // third peer connected, and that peer then took its slot instead of reading `busy`.
    let (addr, rx) = server(Limits { idle_timeout: Duration::from_secs(30), ..small() });
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
    // No UI: the request that reaches the one-slot queue first fills it and times out; the
    // other finds it full. Each connection has its own thread, so either request can be first.
    let (mut ar, mut aw) = client(addr);
    let (mut br, mut bw) = client(addr);
    writeln!(aw, r#"{{"id":1,"method":"m"}}"#).unwrap();
    writeln!(bw, r#"{{"id":2,"method":"m"}}"#).unwrap();
    let a = reply(&mut ar).unwrap();
    let b = reply(&mut br).unwrap();
    assert_eq!((&a["id"], &b["id"]), (&json!(1), &json!(2)));
    let mut errors = [a["error"].as_str().unwrap_or_default(), b["error"].as_str().unwrap_or_default()];
    errors.sort_unstable();
    assert_eq!(errors, ["busy: too many queued requests", "timeout"], "{a} {b}");
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
