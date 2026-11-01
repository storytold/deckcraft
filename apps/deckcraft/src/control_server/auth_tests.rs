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
