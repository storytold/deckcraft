//! Co-editing convergence (protocol 2): two and three sessions run random command sequences
//! concurrently on one shared presentation, their updates delivered in random order and
//! sometimes twice, and must end with identical presentations (the same shared content, schema
//! 1), equal to what a newcomer joining afterwards builds. Typing in the same text boxes, moving,
//! grouping and deleting shapes, adding, moving and deleting slides, per-user Undo and Redo, and
//! chatting (sending, replying, editing, deleting, clearing) are part of the mix.
//!
//! The sessions are connected in memory ([`collab::IN_MEMORY`]): the test is the transport.
//!
//! Knobs: `DECKCRAFT_COLLAB_SEEDS` (default 6), `DECKCRAFT_COLLAB_STEPS` (default 60),
//! `DECKCRAFT_COLLAB_SEED` (replay one), `DECKCRAFT_COLLAB_TRACE` (print every step).

use deckcraft_engine::collab::{self, IN_MEMORY};
use deckcraft_engine::plugin::push::{Incoming, Pushed};
use deckcraft_engine::{Session, sample};
use serde_json::{Value as Json, json};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

fn env_num(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

/// The ids of the current slide's top-level shapes.
fn shapes(s: &Session) -> Vec<u64> {
    s.doc().ok().and_then(|d| d.current_slide().map(|sl| sl.shapes.iter().map(|x| u64::from(x.id.0)).collect())).unwrap_or_default()
}

/// The ids of the current slide's shapes that hold text.
fn text_shapes(s: &Session) -> Vec<(u64, Vec<usize>)> {
    s.doc()
        .ok()
        .and_then(|d| {
            d.current_slide().map(|sl| {
                sl.shapes
                    .iter()
                    .filter(|x| matches!(x.kind, deckcraft_engine::model::ShapeKind::Shape))
                    .filter_map(|x| Some((u64::from(x.id.0), x.text.as_ref()?.paragraphs.iter().map(|p| p.char_len()).collect())))
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// The chat's message ids (only this person's own, not deleted, when `own`).
fn chat_ids(s: &Session, own: bool) -> Vec<String> {
    let me = s.active_room().map(|r| r.user.id.clone()).unwrap_or_default();
    s.doc()
        .map(|d| d.doc.chat.messages.iter().filter(|m| !own || (m.author.id == me && !m.deleted)).map(|m| m.id.clone()).collect())
        .unwrap_or_default()
}

fn slides(s: &Session) -> usize {
    s.doc().map(|d| d.doc.slides.len()).unwrap_or(0)
}

/// A random edit (or a few, as typing is), as a person would make it.
fn generate(rng: &mut Rng, s: &Session, tag: &str) -> Vec<(String, Json)> {
    let n = slides(s);
    let ids = shapes(s);
    let one = |rng: &mut Rng| if ids.is_empty() { json!([]) } else { json!([ids[rng.below(ids.len())]]) };
    let v = match rng.below(36) {
        0 => vec![("slide.new", json!({"layout": rng.pick(&["blank", "titleAndContent", "title"]), "at": rng.below(n + 1)}))],
        1 if n > 1 => vec![("slide.delete", json!({"index": rng.below(n)}))],
        2 if n > 1 => vec![("slide.move", json!({"from": rng.below(n), "to": rng.below(n)}))],
        3 | 4 => vec![("slide.go", json!({"index": rng.below(n.max(1))}))],
        5 | 6 => {
            vec![(
                "shape.insert",
                json!({"preset": rng.pick(&["rect", "ellipse", "star5"]), "rect": [rng.below(700), rng.below(400), 120, 60], "text": format!("{tag}box")}),
            )]
        }
        7 | 8 => vec![(
            "shape.setBounds",
            json!({"ids": one(rng), "x": rng.below(800), "y": rng.below(450), "w": 50 + rng.below(200), "h": 30 + rng.below(100)}),
        )],
        9 => vec![("shape.fill", json!({"ids": one(rng), "color": rng.pick(&["#FF0000", "#00B050", "#3B7DD8"])}))],
        10 => vec![("edit.delete", json!({"ids": one(rng)}))],
        11..=17 => {
            // Typing in a text box: a few keystrokes, each its own command (and update).
            let ts = text_shapes(s);
            if ts.is_empty() {
                vec![]
            } else {
                let (id, paras) = &ts[rng.below(ts.len())];
                let p = rng.below(paras.len().max(1));
                let c = rng.below(paras.get(p).copied().unwrap_or(0) + 1);
                let mut v = vec![("text.edit", json!({"id": id, "at": [p, c]}))];
                for _ in 0..1 + rng.below(4) {
                    let what = *rng.pick(&["x", "y", " ", "\n", "é", "😀"]);
                    v.push(("text.insert", json!({"text": what})));
                }
                if rng.chance(30) {
                    v.push(("text.delete", json!({"dir": "backward"})));
                }
                if rng.chance(20) {
                    v.push(("text.selectAll", json!({})));
                    v.push(("format.bold", json!({})));
                }
                v.push(("text.exit", json!({})));
                v
            }
        }
        18 => vec![("arrange.bringToFront", json!({"ids": one(rng)}))],
        19 if ids.len() >= 2 => vec![("arrange.group", json!({"ids": [ids[0], ids[1]]}))],
        20 => vec![("arrange.ungroup", json!({"ids": one(rng)}))],
        21 => vec![("slide.notes", json!({"text": format!("{tag}notes")}))],
        22 => vec![("edit.duplicate", json!({"ids": one(rng)}))],
        23 => vec![("transition.set", json!({"kind": rng.pick(&["fade", "push", "none"])}))],
        24..=26 => vec![("edit.undo", json!({}))],
        27 => vec![("edit.redo", json!({}))],
        // Chatting: messages sent, replied to, edited and deleted by their authors, the chat
        // cleared now and then.
        30..=32 => {
            let last = chat_ids(s, false).last().cloned();
            let mut p = json!({"text": format!("{tag}hello @Ann 😀")});
            if rng.chance(30)
                && let Some(id) = last
            {
                p["replyTo"] = json!(id);
            }
            vec![("chat.send", p)]
        }
        33 => match chat_ids(s, true).first() {
            Some(id) => vec![("chat.edit", json!({"id": id, "text": format!("{tag}edited")}))],
            None => vec![],
        },
        34 => match chat_ids(s, true).last() {
            Some(id) => vec![("chat.delete", json!({"id": id}))],
            None => vec![],
        },
        35 if rng.chance(25) => vec![("chat.clear", json!({}))],
        _ => vec![("edit.select", json!({"ids": one(rng)}))],
    };
    v.into_iter().map(|(id, p)| (id.to_string(), p)).collect()
}

struct Peer {
    s: Session,
    now: u64,
}

impl Peer {
    fn deliver(&mut self, msg: Json) {
        if trace() {
            eprintln!("  deliver {}", msg.to_string().chars().take(200).collect::<String>());
        }
        self.now += 7;
        self.s.collab_poll_pushed(self.now, vec![Pushed { plugin: IN_MEMORY.into(), incoming: Incoming::Message(msg) }]);
    }
}

/// App → transport messages become transport → app messages for every other peer.
fn route(peers: &mut [Peer], from: usize, queue: &mut Vec<(usize, Json)>, rng: &mut Rng) {
    for msg in peers[from].s.collab_take_outgoing() {
        let method = msg["method"].as_str().unwrap_or("");
        if method == "collab/joined" || method == "collab/leave" {
            continue;
        }
        for to in 0..peers.len() {
            if to != from {
                queue.push((to, msg.clone()));
                if rng.chance(8) {
                    queue.push((to, msg.clone()));
                }
            }
        }
    }
}

fn settle(peers: &mut [Peer], queue: &mut Vec<(usize, Json)>, rng: &mut Rng) {
    for _ in 0..20_000 {
        if queue.is_empty() {
            break;
        }
        let k = rng.below(queue.len());
        let (to, msg) = queue.remove(k);
        peers[to].deliver(msg);
        route(peers, to, queue, rng);
    }
}

fn fp(p: &Peer) -> String {
    p.s.doc().map(|d| collab::fingerprint(&d.doc)).unwrap_or_default()
}

fn run(seed: u64, n: usize, steps: usize) -> Result<(), String> {
    let mut rng = Rng(seed.wrapping_mul(0xA24B_AED4_963E_E407) ^ 0xDECC);
    let mut peers: Vec<Peer> = (0..n).map(|_| Peer { s: Session::new(), now: 1_000 }).collect();
    sample::open_sample(&mut peers[0].s).map_err(|e| e.to_string())?;
    peers[0].s.collab_join_in_memory(json!({"room": "fuzz", "mode": "share"})).map_err(|e| e.to_string())?;
    for p in peers.iter_mut().skip(1) {
        p.s.collab_join_in_memory(json!({"room": "fuzz", "mode": "join"})).map_err(|e| e.to_string())?;
    }
    let mut queue = Vec::new();
    for i in 0..n {
        route(&mut peers, i, &mut queue, &mut rng);
    }
    settle(&mut peers, &mut queue, &mut rng);
    for (i, p) in peers.iter().enumerate() {
        if collab::read_only(&p.s).is_some() {
            return Err(format!("peer {i} is still joining after the first sync"));
        }
    }
    let first = fp(&peers[0]);
    for (i, p) in peers.iter().enumerate().skip(1) {
        if fp(p) != first {
            return Err(format!("peer {i} joined with a different presentation\n{}", diff(&first, &fp(p))));
        }
    }
    let mut log = Vec::new();
    for step in 0..steps {
        let i = rng.below(n);
        if rng.chance(55) || queue.is_empty() {
            let tag = format!("p{i}s{step}-");
            for (id, p) in generate(&mut rng, &peers[i].s, &tag) {
                if trace() {
                    eprintln!("{step}: peer {i} {id} {p}");
                }
                let r = peers[i].s.execute(&id, &p);
                log.push(format!("{step}: peer {i} {id} {p} → {}", if r.is_ok() { "ok".to_string() } else { format!("{:?}", r.err()) }));
                route(&mut peers, i, &mut queue, &mut rng);
                // Others' changes can arrive between keystrokes.
                if rng.chance(20) && !queue.is_empty() {
                    let k = rng.below(queue.len());
                    let (to, msg) = queue.remove(k);
                    peers[to].deliver(msg);
                    route(&mut peers, to, &mut queue, &mut rng);
                }
            }
        } else {
            let k = rng.below(queue.len());
            let (to, msg) = queue.remove(k);
            if trace() {
                eprintln!("{step}: peer {to} gets {}", msg["method"]);
            }
            peers[to].deliver(msg);
            route(&mut peers, to, &mut queue, &mut rng);
        }
    }
    settle(&mut peers, &mut queue, &mut rng);
    // A sync round, as when a transport comes back online.
    for i in 0..n {
        peers[i].deliver(json!({"jsonrpc": "2.0", "method": "collab/status", "params": {"room": "fuzz", "state": "offline"}}));
        peers[i].deliver(json!({"jsonrpc": "2.0", "method": "collab/status", "params": {"room": "fuzz", "state": "online"}}));
        route(&mut peers, i, &mut queue, &mut rng);
    }
    settle(&mut peers, &mut queue, &mut rng);
    let first = fp(&peers[0]);
    for (i, p) in peers.iter().enumerate().skip(1) {
        if fp(p) != first {
            return Err(format!("peer {i} differs from peer 0\n{}\n--- steps\n{}", diff(&first, &fp(p)), log.join("\n")));
        }
    }
    if peers.iter().any(|p| p.s.panics_caught > 0) {
        return Err(format!("a command panicked\n--- steps\n{}", log.join("\n")));
    }
    // A newcomer gets the same presentation.
    let mut late = Peer { s: Session::new(), now: 1_000 };
    late.s.collab_join_in_memory(json!({"room": "fuzz", "mode": "join"})).map_err(|e| e.to_string())?;
    peers.push(late);
    let last = peers.len() - 1;
    route(&mut peers, last, &mut queue, &mut rng);
    settle(&mut peers, &mut queue, &mut rng);
    if fp(&peers[last]) != first {
        return Err(format!("a newcomer differs\n{}", diff(&first, &fp(&peers[last]))));
    }
    Ok(())
}

fn trace() -> bool {
    std::env::var_os("DECKCRAFT_COLLAB_TRACE").is_some()
}

/// Where two fingerprints part (they're one long JSON line and a media line).
fn diff(a: &str, b: &str) -> String {
    let at = a.bytes().zip(b.bytes()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    let cut = |s: &str| -> String { s.chars().skip(at.saturating_sub(300)).take(600).collect() };
    format!("at byte {at}:\n- …{}…\n+ …{}…", cut(a), cut(b))
}

fn seeds() -> Vec<u64> {
    match std::env::var("DECKCRAFT_COLLAB_SEED").ok().and_then(|v| v.trim().parse().ok()) {
        Some(one) => vec![one],
        None => (0..env_num("DECKCRAFT_COLLAB_SEEDS", 6)).collect(),
    }
}

#[test]
fn two_sessions_converge() {
    let steps = env_num("DECKCRAFT_COLLAB_STEPS", 60) as usize;
    for seed in seeds() {
        if let Err(e) = run(seed, 2, steps) {
            panic!("seed {seed} (DECKCRAFT_COLLAB_SEED={seed}): {e}");
        }
    }
}

#[test]
fn three_sessions_converge() {
    let steps = env_num("DECKCRAFT_COLLAB_STEPS", 60) as usize;
    for seed in seeds() {
        if let Err(e) = run(seed + 1_000, 3, steps) {
            panic!("seed {} (DECKCRAFT_COLLAB_SEED={}): {e}", seed + 1_000, seed + 1_000);
        }
    }
}

/// Two people typing in the same text box at once both keep what they typed, and each one's
/// Undo takes back only their own typing.
#[test]
fn typing_in_one_text_box_merges_and_undo_is_per_user() {
    let mut a = Peer { s: Session::new(), now: 1_000 };
    let mut b = Peer { s: Session::new(), now: 1_000 };
    a.s.execute("file.new", &json!({})).unwrap();
    a.s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 300, 80], "text": "Hello"})).unwrap();
    let id = shapes(&a.s)[shapes(&a.s).len() - 1];
    a.s.collab_join_in_memory(json!({"room": "r", "mode": "share"})).unwrap();
    b.s.collab_join_in_memory(json!({"room": "r", "mode": "join"})).unwrap();
    let mut peers = vec![a, b];
    let mut rng = Rng(1);
    let mut queue = Vec::new();
    route(&mut peers, 0, &mut queue, &mut rng);
    route(&mut peers, 1, &mut queue, &mut rng);
    settle(&mut peers, &mut queue, &mut rng);
    assert!(collab::read_only(&peers[1].s).is_none(), "joined");
    // Concurrent: A types at the end, B at the start; nothing delivered in between.
    peers[0].s.execute("text.edit", &json!({"id": id, "at": [0, 5]})).unwrap();
    for c in [" ", "w", "o", "r", "l", "d"] {
        peers[0].s.execute("text.insert", &json!({"text": c})).unwrap();
    }
    peers[1].s.execute("text.edit", &json!({"id": id, "at": [0, 0]})).unwrap();
    peers[1].s.execute("text.insert", &json!({"text": "Oh, "})).unwrap();
    route(&mut peers, 0, &mut queue, &mut rng);
    route(&mut peers, 1, &mut queue, &mut rng);
    settle(&mut peers, &mut queue, &mut rng);
    let text = |p: &Peer| {
        p.s.doc().unwrap().current_slide().unwrap().shape(deckcraft_engine::model::ShapeId(id as u32)).unwrap().text.as_ref().unwrap().text()
    };
    assert_eq!(text(&peers[0]), "Oh, Hello world");
    assert_eq!(text(&peers[1]), "Oh, Hello world");
    // B's caret stayed after what B typed, and A's after A's, whatever the other typed.
    assert_eq!(peers[1].s.doc().unwrap().selection.text.as_ref().unwrap().caret, (0, 4));
    assert_eq!(peers[0].s.doc().unwrap().selection.text.as_ref().unwrap().caret, (0, 15));
    // B's Undo takes back B's typing only.
    let r = peers[1].s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(r["perUser"], true);
    route(&mut peers, 1, &mut queue, &mut rng);
    settle(&mut peers, &mut queue, &mut rng);
    assert_eq!(text(&peers[0]), "Hello world");
    assert_eq!(text(&peers[1]), "Hello world");
    assert!(peers[1].s.execute("edit.redo", &json!({})).is_ok());
}

/// A viewer's and a joiner's edits never leave, and the room's document comes back.
#[test]
fn viewers_and_joiners_are_read_only() {
    let mut a = Session::new();
    a.execute("file.new", &json!({})).unwrap();
    a.collab_join_in_memory(json!({"room": "r", "mode": "share"})).unwrap();
    let mut v = Session::new();
    v.collab_join_in_memory(json!({"room": "r", "mode": "join", "role": "view"})).unwrap();
    assert_eq!(collab::read_only(&v).as_deref(), Some("Joining…"));
    assert!(v.execute("slide.new", &json!({})).unwrap_err().to_string().contains("Joining"));
    // The first state arrives.
    let mut rng = Rng(2);
    let mut peers = vec![Peer { s: a, now: 1_000 }, Peer { s: v, now: 1_000 }];
    let mut queue = Vec::new();
    route(&mut peers, 0, &mut queue, &mut rng);
    route(&mut peers, 1, &mut queue, &mut rng);
    settle(&mut peers, &mut queue, &mut rng);
    assert!(collab::read_only(&peers[1].s).unwrap().contains("view only"));
    assert!(peers[1].s.execute("slide.new", &json!({})).is_err());
    assert_eq!(fp(&peers[0]), fp(&peers[1]));
    // Leaving makes it an ordinary presentation with everything in it.
    peers[1].s.execute("collab.leave", &json!({})).unwrap();
    assert!(peers[1].s.execute("slide.new", &json!({})).is_ok());
}
