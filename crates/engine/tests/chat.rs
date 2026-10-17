//! The document chat (craft-collab `docs/chat.md`): the `chat.*` commands and their limits, chat
//! changes outside undo but making the presentation modified, the chat saved in `.pptx` and
//! `.deckcraft` files, and in live sessions: sharing seeds the room with the file's chat, joining
//! brings the room's chat in, concurrent chatting by two and three people converges in one order,
//! edits, deletions and clearing reach everyone, Undo never removes a message, viewers chat,
//! "typing" travels in awareness, and an encrypted room carries the chat as ciphertext.

use std::sync::Arc;

use deckcraft_engine::collab::{self, IN_MEMORY};
use deckcraft_engine::collab_core::b64_decode;
use deckcraft_engine::collab_core::crypto::Key;
use deckcraft_engine::model::chat::{self, Chat, ChatAuthor, ChatMessage};
use deckcraft_engine::plugin::push::{Incoming, Pushed};
use deckcraft_engine::{Session, sample};
use serde_json::{Value as Json, json};

fn messages(s: &Session) -> Vec<ChatMessage> {
    s.doc().unwrap().doc.chat.messages.clone()
}

fn texts(s: &Session) -> Vec<String> {
    messages(s).iter().map(|m| if m.deleted { "(deleted)".to_string() } else { m.text.clone() }).collect()
}

#[test]
fn commands_send_list_edit_delete_clear_and_export() {
    let mut s = Session::with_new();
    s.execute("collab.setUser", &json!({"name": "Ann Lee", "color": "#3B7DD8"})).unwrap();
    assert!(!s.doc().unwrap().is_dirty());
    let a = s.execute("chat.send", &json!({"text": "Slide 2 needs a chart"})).unwrap();
    let a = a["id"].as_str().unwrap().to_string();
    assert_eq!(a.len(), 8);
    assert!(a.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_lowercase()));
    assert!(s.doc().unwrap().is_dirty(), "a message makes the presentation modified");
    let m = &messages(&s)[0];
    assert_eq!(m.author.name, "Ann Lee");
    assert_eq!(m.author.color, "#3B7DD8");
    assert!(m.at > 1_600_000_000_000, "the sender's clock in milliseconds");
    let b = s.execute("chat.send", &json!({"text": "On it\nwith two lines", "replyTo": a})).unwrap()["id"].as_str().unwrap().to_string();
    assert_eq!(messages(&s)[1].reply_to.as_deref(), Some(a.as_str()));

    // chat.list: newest last, since an id or a time, limited.
    let l = s.execute("chat.list", &json!({})).unwrap();
    assert_eq!(l["count"], 2);
    assert_eq!(l["messages"][1]["text"], "On it\nwith two lines");
    assert_eq!(l["messages"][0]["author"]["name"], "Ann Lee");
    let l = s.execute("chat.list", &json!({"since": a})).unwrap();
    assert_eq!(l["messages"].as_array().unwrap().len(), 1);
    let l = s.execute("chat.list", &json!({"limit": 1})).unwrap();
    assert_eq!(l["messages"][0]["id"], json!(b));
    let l = s.execute("chat.list", &json!({"since": 0})).unwrap();
    assert_eq!(l["messages"].as_array().unwrap().len(), 2);
    assert!(s.execute("chat.list", &json!({"since": [1]})).is_err());

    // Edit and delete: the author only.
    s.execute("chat.edit", &json!({"id": a, "text": "Slide 2 needs a bar chart"})).unwrap();
    assert!(messages(&s)[0].edited.is_some());
    assert_eq!(messages(&s)[0].text, "Slide 2 needs a bar chart");
    s.execute("chat.delete", &json!({"id": b})).unwrap();
    assert!(messages(&s)[1].deleted && messages(&s)[1].text.is_empty(), "Message deleted stays");
    assert!(s.execute("chat.edit", &json!({"id": b, "text": "back"})).is_err(), "a deleted message can't be edited");
    // Someone else's message (as a file or a peer brings it).
    {
        let st = s.doc_mut().unwrap();
        let mut c = (*st.doc.chat).clone();
        c.messages.push(ChatMessage {
            id: "other001".into(),
            author: ChatAuthor { id: "u-bob".into(), name: "Bob".into(), color: "#D8553B".into() },
            at: 5,
            text: "Mine".into(),
            ..Default::default()
        });
        st.doc = Arc::new(deckcraft_engine::model::Presentation { chat: Arc::new(c), ..(*st.doc).clone() });
    }
    let e = s.execute("chat.edit", &json!({"id": "other001", "text": "Hacked"})).unwrap_err().to_string();
    assert!(e.contains("only the author"), "{e}");
    assert!(s.execute("chat.delete", &json!({"id": "other001"})).is_err());

    // Export as text and as JSON.
    let t = s.execute("chat.export", &json!({})).unwrap();
    let text = t["text"].as_str().unwrap();
    assert!(text.contains("Ann Lee: Slide 2 needs a bar chart (edited)"), "{text}");
    assert!(text.contains("Ann Lee (replying to Ann Lee): Message deleted"), "{text}");
    assert!(text.contains("Bob: Mine"), "{text}");
    let j = s.execute("chat.export", &json!({"format": "json"})).unwrap();
    let parsed: Json = serde_json::from_str(j["text"].as_str().unwrap()).unwrap();
    assert_eq!(parsed["version"], 1);
    assert_eq!(parsed["messages"].as_array().unwrap().len(), 3);
    assert!(s.execute("chat.export", &json!({"format": "docx"})).is_err());

    // Clear: every message goes, clearedAt is set.
    let r = s.execute("chat.clear", &json!({})).unwrap();
    assert_eq!(r["removed"], 3);
    let l = s.execute("chat.list", &json!({})).unwrap();
    assert_eq!(l["count"], 0);
    assert!(l["clearedAt"].as_i64().is_some());
}

#[test]
fn limits_and_bad_params() {
    let mut s = Session::with_new();
    for p in [json!({}), json!({"text": ""}), json!({"text": "   \n "}), json!({"text": 5}), json!({"text": "x".repeat(chat::MAX_TEXT + 1)})] {
        assert!(s.execute("chat.send", &p).is_err(), "{p}");
    }
    assert!(s.execute("chat.send", &json!({"text": "x".repeat(chat::MAX_TEXT)})).is_ok(), "4,000 characters fit");
    assert!(s.execute("chat.send", &json!({"text": "😀".repeat(chat::MAX_TEXT / 2)})).is_ok(), "counted in UTF-16 units");
    assert!(s.execute("chat.send", &json!({"text": "😀".repeat(chat::MAX_TEXT / 2 + 1)})).is_err());
    assert!(s.execute("chat.send", &json!({"text": "hi", "replyTo": "nope0000"})).is_err(), "a reply to nothing");
    assert!(s.execute("chat.edit", &json!({"id": "nope0000", "text": "x"})).is_err());
    assert!(s.execute("chat.delete", &json!({})).is_err());
    // At 10,000 messages the chat is full.
    {
        let st = s.doc_mut().unwrap();
        let author = ChatAuthor { id: "u".into(), name: "U".into(), color: "#000000".into() };
        let messages = (0..chat::MAX_MESSAGES)
            .map(|i| ChatMessage { id: format!("{i:08}"), author: author.clone(), at: i as i64, text: "m".into(), ..Default::default() })
            .collect();
        st.doc = Arc::new(deckcraft_engine::model::Presentation { chat: Arc::new(Chat { cleared_at: None, messages }), ..(*st.doc).clone() });
    }
    let e = s.execute("chat.send", &json!({"text": "one more"})).unwrap_err().to_string();
    assert!(e.contains(chat::FULL), "{e}");
    s.execute("chat.clear", &json!({})).unwrap();
    s.execute("chat.send", &json!({"text": "one more"})).unwrap();
    // No presentation: refused, not a crash.
    let mut empty = Session::new();
    for id in ["chat.list", "chat.send", "chat.edit", "chat.delete", "chat.clear", "chat.export", "chat.typing"] {
        assert!(empty.execute(id, &json!({"text": "x", "id": "x"})).is_err(), "{id}");
    }
}

#[test]
fn chat_changes_are_not_undo_steps() {
    let mut s = Session::with_new();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 100, 50]})).unwrap();
    let shapes = |s: &Session| s.doc().unwrap().current_slide().unwrap().shapes.len();
    let n = shapes(&s);
    s.execute("chat.send", &json!({"text": "Added a box"})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(shapes(&s), n - 1, "Undo undoes the shape, not the message");
    assert_eq!(texts(&s), ["Added a box"]);
    s.execute("chat.send", &json!({"text": "Removed it"})).unwrap();
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(shapes(&s), n);
    assert_eq!(texts(&s), ["Added a box", "Removed it"]);
}

#[test]
fn the_chat_is_saved_in_pptx_and_deckcraft_files() {
    let dir = std::env::temp_dir().join(format!("deckcraft-chat-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for ext in ["pptx", "deckcraft", "pptm"] {
        let mut s = Session::with_new();
        let a = s.execute("chat.send", &json!({"text": format!("Saved in .{ext}")})).unwrap()["id"].as_str().unwrap().to_string();
        s.execute("chat.send", &json!({"text": "A reply", "replyTo": a})).unwrap();
        let path = dir.join(format!("chat.{ext}"));
        s.execute("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
        assert!(!s.doc().unwrap().is_dirty());
        let before = (*s.doc().unwrap().doc.chat).clone();
        let mut t = Session::new();
        t.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
        assert_eq!(*t.doc().unwrap().doc.chat, before, ".{ext}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- live sessions

struct Peer {
    s: Session,
    now: u64,
}

impl Peer {
    fn new() -> Peer {
        Peer { s: Session::new(), now: 1_000 }
    }
    fn deliver(&mut self, msg: Json) {
        self.now += 7;
        self.s.collab_poll_pushed(self.now, vec![Pushed { plugin: IN_MEMORY.into(), incoming: Incoming::Message(msg) }]);
    }
    fn run(&mut self, id: &str, p: Json) -> Json {
        self.s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
    }
}

/// Everything sent so far reaches the others, in order (`rev`: newest first), until quiet.
fn settle(peers: &mut [Peer], rev: bool) -> Vec<Json> {
    let mut sent = Vec::new();
    for _ in 0..200 {
        let mut queue = Vec::new();
        for (from, p) in peers.iter_mut().enumerate() {
            for m in p.s.collab_take_outgoing() {
                sent.push(m.clone());
                queue.push((from, m));
            }
        }
        if queue.is_empty() {
            break;
        }
        if rev {
            queue.reverse();
        }
        for (from, m) in queue {
            let method = m["method"].as_str().unwrap_or("");
            if method == "collab/joined" || method == "collab/leave" || method.is_empty() {
                continue;
            }
            for (to, p) in peers.iter_mut().enumerate() {
                if to != from {
                    p.deliver(m.clone());
                }
            }
        }
    }
    sent
}

fn user(name: &str) -> Json {
    json!({"id": format!("u-{}", name.to_lowercase()), "name": name, "color": "#3B7DD8"})
}

fn room(n: usize, key: Option<&str>) -> Vec<Peer> {
    let mut peers: Vec<Peer> = (0..n).map(|_| Peer::new()).collect();
    sample::open_sample(&mut peers[0].s).unwrap();
    let names = ["Ann", "Bob", "Cy"];
    let mut p = json!({"room": "chat", "mode": "share", "user": user(names[0])});
    if let Some(k) = key {
        p["key"] = json!(k);
    }
    peers[0].s.collab_join_in_memory(p.clone()).unwrap();
    for (i, peer) in peers.iter_mut().enumerate().skip(1) {
        p["mode"] = json!("join");
        p["user"] = user(names[i % names.len()]);
        peer.s.collab_join_in_memory(p.clone()).unwrap();
    }
    settle(&mut peers, false);
    for p in &peers {
        assert!(collab::read_only(&p.s).is_none(), "joined");
    }
    peers
}

fn same_chat(peers: &[Peer]) -> Vec<String> {
    let first = messages(&peers[0].s);
    for (i, p) in peers.iter().enumerate().skip(1) {
        assert_eq!(messages(&p.s), first, "peer {i}'s chat");
        assert_eq!(p.s.doc().unwrap().doc.chat.cleared_at, peers[0].s.doc().unwrap().doc.chat.cleared_at);
        assert_eq!(collab::fingerprint(&p.s.doc().unwrap().doc), collab::fingerprint(&peers[0].s.doc().unwrap().doc));
    }
    texts(&peers[0].s)
}

#[test]
fn sharing_seeds_the_room_with_the_files_chat_and_joining_brings_it_in() {
    let mut a = Peer::new();
    sample::open_sample(&mut a.s).unwrap();
    a.run("chat.send", json!({"text": "Left in the file before sharing"}));
    a.s.collab_join_in_memory(json!({"room": "seed", "mode": "share", "user": user("Ann")})).unwrap();
    let mut b = Peer::new();
    b.s.collab_join_in_memory(json!({"room": "seed", "mode": "join", "user": user("Bob")})).unwrap();
    let mut peers = vec![a, b];
    settle(&mut peers, false);
    assert_eq!(same_chat(&peers), ["Left in the file before sharing"]);
    assert!(!peers[1].s.doc().unwrap().is_dirty(), "a joined presentation starts clean");
    // A newcomer after more chatting gets it all.
    peers[1].run("chat.send", json!({"text": "Hi Ann"}));
    settle(&mut peers, false);
    assert!(peers[0].s.doc().unwrap().is_dirty(), "others' messages make the presentation modified");
    let mut c = Peer::new();
    c.s.collab_join_in_memory(json!({"room": "seed", "mode": "join", "user": user("Cy")})).unwrap();
    peers.push(c);
    settle(&mut peers, false);
    assert_eq!(same_chat(&peers), ["Left in the file before sharing", "Hi Ann"]);
}

#[test]
fn concurrent_chatting_converges_two_and_three_people() {
    for n in [2, 3] {
        for rev in [false, true] {
            let mut peers = room(n, None);
            // Everyone sends at once, nothing delivered in between.
            for round in 0..4 {
                for (i, p) in peers.iter_mut().enumerate() {
                    p.run("chat.send", json!({"text": format!("p{i} r{round}")}));
                }
            }
            settle(&mut peers, rev);
            let order = same_chat(&peers);
            assert_eq!(order.len(), 4 * n);
            // Each person's own messages stay in the order they sent them.
            for i in 0..n {
                let mine: Vec<&String> = order.iter().filter(|t| t.starts_with(&format!("p{i} "))).collect();
                let want: Vec<String> = (0..4).map(|r| format!("p{i} r{r}")).collect();
                assert_eq!(mine, want.iter().collect::<Vec<_>>());
            }
            // Replies, edits and deletions by their authors reach everyone.
            let first = messages(&peers[0].s)[0].id.clone();
            peers[n - 1].run("chat.send", json!({"text": "a reply", "replyTo": first}));
            let own0: Vec<String> = messages(&peers[0].s).iter().filter(|m| m.author.id == "u-ann").map(|m| m.id.clone()).collect();
            peers[0].run("chat.edit", json!({"id": own0[0], "text": "p0 r0, edited"}));
            peers[0].run("chat.delete", json!({"id": own0[1]}));
            settle(&mut peers, rev);
            let order = same_chat(&peers);
            assert!(order.contains(&"p0 r0, edited".to_string()));
            assert!(order.contains(&"(deleted)".to_string()));
            let last = messages(&peers[1].s).last().cloned().unwrap();
            assert_eq!(last.reply_to.as_deref(), Some(first.as_str()));
            // Only the author edits: another person's edit is refused locally.
            assert!(peers[1].s.execute("chat.edit", &json!({"id": own0[0], "text": "nope"})).is_err());
        }
    }
}

#[test]
fn clearing_reaches_everyone_and_messages_sent_meanwhile_survive() {
    let mut peers = room(3, None);
    peers[0].run("chat.send", json!({"text": "one"}));
    peers[1].run("chat.send", json!({"text": "two"}));
    settle(&mut peers, false);
    // Bob clears while Cy, not knowing yet, sends one more.
    peers[1].run("chat.clear", json!({}));
    peers[2].run("chat.send", json!({"text": "three"}));
    settle(&mut peers, true);
    assert_eq!(same_chat(&peers), ["three"]);
    assert!(peers[0].s.doc().unwrap().doc.chat.cleared_at.is_some());
    peers[0].run("chat.clear", json!({}));
    settle(&mut peers, false);
    assert!(same_chat(&peers).is_empty());
}

#[test]
fn undo_in_a_session_never_removes_a_message() {
    let mut peers = room(2, None);
    peers[0].run("shape.insert", json!({"preset": "rect", "rect": [10, 10, 100, 50]}));
    peers[0].run("chat.send", json!({"text": "Added a box"}));
    settle(&mut peers, false);
    let shapes = |p: &Peer| p.s.doc().unwrap().current_slide().unwrap().shapes.len();
    let n = shapes(&peers[1]);
    let r = peers[0].run("edit.undo", json!({}));
    assert_eq!(r["perUser"], true);
    settle(&mut peers, false);
    assert_eq!(shapes(&peers[1]), n - 1, "the shape is undone for everyone");
    assert_eq!(same_chat(&peers), ["Added a box"], "the message stays");
    // A message alone is no undo step.
    peers[1].run("chat.send", json!({"text": "Only chat"}));
    assert!(peers[1].s.execute("edit.undo", &json!({})).is_err(), "nothing of Bob's to undo");
}

#[test]
fn viewers_chat_and_typing_travels_in_awareness() {
    let mut a = Peer::new();
    sample::open_sample(&mut a.s).unwrap();
    a.s.collab_join_in_memory(json!({"room": "v", "mode": "share", "user": user("Ann")})).unwrap();
    let mut b = Peer::new();
    b.s.collab_join_in_memory(json!({"room": "v", "mode": "join", "role": "view", "user": user("Bob")})).unwrap();
    let mut peers = vec![a, b];
    settle(&mut peers, false);
    peers[1].run("chat.send", json!({"text": "Viewer here"}));
    assert!(peers[1].s.execute("slide.new", &json!({})).is_err(), "a viewer doesn't edit the slides");
    settle(&mut peers, false);
    assert_eq!(same_chat(&peers), ["Viewer here"]);
    // Typing.
    peers[0].run("chat.typing", json!({"typing": true}));
    peers[0].now += 100;
    let now = peers[0].now;
    peers[0].s.collab_poll_pushed(now, vec![]);
    settle(&mut peers, false);
    let typing: Vec<String> = peers[1].s.active_room().unwrap().peers().iter().filter(|p| p.typing).map(|p| p.user.name.clone()).collect();
    assert_eq!(typing, ["Ann"]);
    peers[0].run("chat.typing", json!({"typing": false}));
    peers[0].now += 100;
    let now = peers[0].now;
    peers[0].s.collab_poll_pushed(now, vec![]);
    settle(&mut peers, false);
    assert!(peers[1].s.active_room().unwrap().peers().iter().all(|p| !p.typing));
}

#[test]
fn an_encrypted_room_carries_the_chat_as_ciphertext() {
    let key = Key::generate().unwrap().to_base64();
    let mut peers = room(2, Some(&key));
    peers[0].run("chat.send", json!({"text": "Top secret plan"}));
    let sent = settle(&mut peers, false);
    assert_eq!(same_chat(&peers), ["Top secret plan"]);
    let mut updates = 0;
    for m in &sent {
        if let Some(u) = m["params"]["update"].as_str() {
            updates += 1;
            let bytes = b64_decode(u).unwrap();
            assert!(!bytes.windows(6).any(|w| w == b"secret"), "plaintext on the wire: {m}");
        }
    }
    assert!(updates > 0);
}
