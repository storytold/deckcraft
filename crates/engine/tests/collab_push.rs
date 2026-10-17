//! Protocol 2 end to end through a real plugin process: the push channel and co-editing. The
//! transport is this test binary started again with `--serve-transport` (no libtest harness, so
//! standard output is the protocol's alone). It relays between two rooms, `relay:a` and
//! `relay:b`, as a network would between two people: what the app sends for one arrives at the
//! other, with `from`. Two sessions in this process each join one room.
//!
//! Checked: `initialize` with `push`/`collab`; a plugin command answering `collab.join`; notifications
//! the transport sends on its own (status, relayed sync and updates) reaching each session's
//! loop; the transport's own requests (`collab/snapshot` with the file, `host/log`) answered;
//! the joiner getting the document; edits both ways; per-user undo travelling; leaving.
#![allow(clippy::disallowed_methods)] // the clock, off wasm

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

use deckcraft_engine::Session;
use deckcraft_engine::collab;
use deckcraft_engine::plugin::push::{self, Incoming};
use serde_json::{Value as Json, json};

const PLUGIN: &str = "org.deckcraft.test-relay";

fn main() {
    if std::env::args().any(|a| a == "--serve-transport") {
        serve();
        return;
    }
    let dir = std::env::temp_dir().join(format!("deckcraft-engine-relay-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let exe = std::env::current_exe().unwrap();
    let manifest = json!({
        "id": PLUGIN, "name": "Relay", "run": {"command": exe.display().to_string(), "args": ["--serve-transport"]},
        "commands": [{"id": "share", "label": "Share"}],
        "collab": {"transport": "relay", "label": "Live (test relay)"},
    });
    std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
    deckcraft_engine::plugin::load(&dir).unwrap();
    two_sessions_edit_through_the_transport();
    println!("test two_sessions_edit_through_the_transport ... ok");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- the transport

fn serve() {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    let mut send = |m: Json| {
        let _ = writeln!(out, "{m}");
        let _ = out.flush();
    };
    let mut clients: HashMap<String, u64> = HashMap::new();
    let mut asked_snapshot = false;
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<Json>(&line) else { continue };
        let method = msg["method"].as_str().unwrap_or("").to_string();
        let p = msg["params"].clone();
        if let Some(id) = msg.get("id").filter(|_| !method.is_empty()) {
            // The app's requests.
            let result = match method.as_str() {
                "initialize" => json!({"protocolVersion": 1, "host": p["host"]}),
                "commands/run" => {
                    json!({"collab": {"join": {"room": "relay:a", "mode": "share", "user": {"id": "u-a", "name": "Ann", "color": "#3B7DD8"}}}})
                }
                _ => json!({}),
            };
            // A notification of our own in the middle of a request: it must reach the app's loop.
            if method == "commands/run" {
                send(json!({"jsonrpc": "2.0", "method": "collab/status", "params": {"room": "relay:none", "state": "online"}}));
            }
            send(json!({"jsonrpc": "2.0", "id": id, "result": result}));
            continue;
        }
        if method.is_empty() {
            // An answer to one of our requests.
            if msg["id"] == "snap" {
                let r = &msg["result"];
                let ok = r["update"].as_str().is_some_and(|u| !u.is_empty())
                    && r["file"].as_str().is_some_and(|f| f.len() > 100)
                    && r["name"].as_str().is_some_and(|n| n.ends_with(".pptx"));
                send(
                    json!({"jsonrpc": "2.0", "method": "collab/status", "params": {"room": "relay:a", "state": "online", "message": format!("snapshot ok: {ok}")}}),
                );
            }
            continue;
        }
        if method == "shutdown" {
            break;
        }
        let room = p["room"].as_str().unwrap_or("").to_string();
        let other = match room.as_str() {
            "relay:a" => "relay:b",
            "relay:b" => "relay:a",
            _ => continue,
        };
        match method.as_str() {
            "collab/joined" => {
                clients.insert(room.clone(), p["clientId"].as_u64().unwrap_or(0));
                send(json!({"jsonrpc": "2.0", "method": "collab/status", "params": {"room": room, "state": "connecting"}}));
                send(json!({"jsonrpc": "2.0", "method": "collab/status", "params": {"room": room, "state": "online", "peers": []}}));
                send(json!({"jsonrpc": "2.0", "id": 7, "method": "host/log", "params": {"text": format!("joined {room}")}}));
            }
            "collab/sync1" | "collab/sync2" | "collab/update" | "collab/awareness" => {
                let mut q = p.clone();
                q["room"] = json!(other);
                q["from"] = json!(clients.get(&room).copied().unwrap_or(0));
                if method == "collab/sync2" {
                    q.as_object_mut().unwrap().remove("to");
                }
                send(json!({"jsonrpc": "2.0", "method": method, "params": q}));
                if method == "collab/update" && room == "relay:a" && !asked_snapshot {
                    asked_snapshot = true;
                    send(json!({"jsonrpc": "2.0", "id": "snap", "method": "collab/snapshot", "params": {"room": "relay:a", "file": true}}));
                }
            }
            "collab/leave" => {
                send(
                    json!({"jsonrpc": "2.0", "method": "collab/status", "params": {"room": other, "state": "online", "peers": [], "message": "the other left"}}),
                );
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------- the test

/// Hands each session the messages for its room.
fn pump(a: &mut Session, b: &mut Session, now: u64) {
    let mut for_a = Vec::new();
    let mut for_b = Vec::new();
    for p in push::take() {
        let room = match &p.incoming {
            Incoming::Message(m) => m["params"]["room"].as_str().unwrap_or("relay:a").to_string(),
            _ => "relay:a".into(),
        };
        if room == "relay:b" { for_b.push(p) } else { for_a.push(p) }
    }
    a.collab_poll_pushed(now, for_a);
    b.collab_poll_pushed(now, for_b);
}

fn wait(a: &mut Session, b: &mut Session, what: &str, mut done: impl FnMut(&Session, &Session) -> bool) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        let now = start.elapsed().as_millis() as u64 + 1_000;
        pump(a, b, now);
        if done(a, b) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

/// The texts of the first slide's shapes, and how many slides there are.
fn texts(s: &Session) -> (Vec<String>, usize) {
    let Some(d) = s.active() else { return (vec![], 0) };
    let first = d.doc.slides.first().map(|sl| sl.shapes.iter().filter_map(|x| x.text.as_ref().map(|t| t.text())).collect()).unwrap_or_default();
    (first, d.doc.slides.len())
}

fn has(s: &Session, text: &str) -> bool {
    texts(s).0.iter().any(|t| t == text)
}

fn two_sessions_edit_through_the_transport() {
    let mut a = Session::new();
    a.execute("file.new", &json!({})).unwrap();
    let r = a.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 300, 80], "text": "Item"})).unwrap();
    let id = r["id"].as_u64().unwrap();
    // A plugin command answers `collab.join`: returned to the caller, who joins.
    let r = a.execute("plugins.run", &json!({"plugin": PLUGIN, "command": "share"})).unwrap();
    let mut join = r["collab"]["join"].clone();
    assert_eq!(join["room"], "relay:a", "{r}");
    join["plugin"] = json!(PLUGIN);
    a.execute("collab.join", &join).unwrap();
    let mut b = Session::new();
    b.execute("collab.join", &json!({"plugin": PLUGIN, "room": "relay:b", "mode": "join", "user": {"id": "u-b", "name": "Bob", "color": "#D8513B"}}))
        .unwrap();
    assert_eq!(collab::read_only(&b).as_deref(), Some("Joining…"));
    assert!(b.execute("slide.new", &json!({})).is_err(), "read-only while joining");

    wait(&mut a, &mut b, "the joiner to get the presentation", |_, b| collab::read_only(b).is_none() && has(b, "Item"));
    let slides = texts(&a).1;
    assert_eq!(texts(&b).1, slides);

    // Edits both ways: Ann types in the box, Bob adds a slide.
    a.execute("text.edit", &json!({"id": id, "at": [0, 4]})).unwrap();
    a.execute("text.insert", &json!({"text": "s"})).unwrap();
    b.execute("slide.new", &json!({"layout": "blank", "at": 1})).unwrap();
    wait(&mut a, &mut b, "the edits to cross", |a, b| has(b, "Items") && texts(a).1 == slides + 1 && texts(b).1 == slides + 1);
    // The transport asked for a snapshot with the file; the answer was complete.
    wait(&mut a, &mut b, "the snapshot answer", |a, _| a.active_room().and_then(|r| r.message.clone()).as_deref() == Some("snapshot ok: true"));
    // Awareness: each sees the other.
    wait(&mut a, &mut b, "awareness", |a, b| {
        a.active_room().is_some_and(|r| r.peers().iter().any(|p| p.user.name == "Bob"))
            && b.active_room().is_some_and(|r| r.peers().iter().any(|p| p.user.name == "Ann"))
    });

    // Per-user undo: Bob's undo removes only his slide.
    b.execute("edit.undo", &json!({})).unwrap();
    wait(&mut a, &mut b, "the undo", |a, _| texts(a).1 == slides && has(a, "Items"));
    wait(&mut a, &mut b, "the same presentation", |a, b| {
        collab::fingerprint(&a.active().unwrap().doc) == collab::fingerprint(&b.active().unwrap().doc)
    });

    // Leaving: the presentation stays, edits stay local.
    a.execute("collab.leave", &json!({})).unwrap();
    wait(&mut a, &mut b, "the leave", |_, b| b.active_room().and_then(|r| r.message.clone()).as_deref() == Some("the other left"));
    a.execute("slide.new", &json!({})).unwrap();
    assert!(has(&a, "Items"));
    for _ in 0..20 {
        pump(&mut a, &mut b, 99_000);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(texts(&b).1, slides);
}
