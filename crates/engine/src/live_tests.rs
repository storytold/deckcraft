//! The craft-live client against a fake room that behaves as craft-live's (`docs/design.md` §4–§6,
//! §9): a plain Yjs document, the seed lease, joiners held until the room is seeded, updates
//! relayed, awareness relayed, `save` asked of the editor who edited last, `reseed` when the
//! seeder leaves early. The HTTP side (the file, `prepare`, the upload) is answered by the tests.

use std::collections::BTreeMap;

use serde_json::{Value as Json, json};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Doc, ReadTxn, StateVector, Transact, Update};

use super::wire::Frame;
use super::*;
use crate::Session;

// ------------------------------------------------------------------ the fake room

#[derive(Default)]
struct Conn {
    open: bool,
    synced: bool,
    pending_sync1: Option<Vec<u8>>,
    client_id: Option<u64>,
    view: bool,
    /// Frames for this client, oldest first.
    out: Vec<Vec<u8>>,
}

struct Room {
    doc: Doc,
    seeded: bool,
    lease: Option<usize>,
    conns: BTreeMap<usize, Conn>,
    /// Frames each client sent, decoded.
    got: BTreeMap<usize, Vec<Frame>>,
    last_editor: Option<usize>,
    next_save: u64,
    dropped_viewer_updates: usize,
}

impl Room {
    fn new() -> Room {
        Room {
            doc: Doc::new(),
            seeded: false,
            lease: None,
            conns: BTreeMap::new(),
            got: BTreeMap::new(),
            last_editor: None,
            next_save: 1,
            dropped_viewer_updates: 0,
        }
    }

    fn send(&mut self, to: usize, f: &Frame) {
        if let Some(c) = self.conns.get_mut(&to)
            && c.open
        {
            c.out.push(f.encode());
        }
    }

    fn control(&mut self, to: usize, v: Json) {
        self.send(to, &Frame::control(&v));
    }

    fn sv(&self) -> Vec<u8> {
        self.doc.transact().state_vector().encode_v1()
    }

    fn diff(&self, sv: &[u8]) -> Vec<u8> {
        let sv = StateVector::decode_v1(sv).unwrap_or_default();
        self.doc.transact().encode_diff_v1(&sv)
    }

    fn empty(&self) -> bool {
        self.doc.transact().state_vector().is_empty()
    }

    fn connect(&mut self, who: usize, view: bool) {
        let seeder = self.lease == Some(who);
        let synced = self.seeded || seeder;
        self.conns.insert(who, Conn { open: true, synced, view, ..Default::default() });
        self.control(who, json!({"type": "welcome", "room": "nc:cloud:42", "role": if view { "view" } else { "edit" }, "seeded": self.seeded, "seed": seeder && !self.seeded, "modified": false, "fileName": "Talk.pptx"}));
        if synced {
            let sv = self.sv();
            self.send(who, &Frame::Sync1(sv));
        }
        self.peers();
    }

    fn peers(&mut self) {
        let ids: Vec<usize> = self.conns.iter().filter(|(_, c)| c.open).map(|(i, _)| *i).collect();
        for to in &ids {
            let others: Vec<Json> = ids
                .iter()
                .filter(|i| *i != to)
                .map(|i| json!({"user": {"id": format!("u{i}"), "name": format!("Person {i}"), "color": "#3B7DD8"}, "role": "edit", "kind": "web", "clientId": self.conns.get(i).and_then(|c| c.client_id)}))
                .collect();
            self.control(*to, json!({"type": "peers", "peers": others}));
        }
    }

    fn disconnect(&mut self, who: usize) {
        if let Some(c) = self.conns.get_mut(&who) {
            c.open = false;
        }
        if self.lease == Some(who) && !self.seeded {
            self.lease = None;
            let waiting: Vec<usize> = self.conns.iter().filter(|(_, c)| c.open).map(|(i, _)| *i).collect();
            for w in waiting {
                self.control(w, json!({"type": "reseed", "message": "The person loading the file left before it was ready. Open the file again."}));
            }
        }
        self.peers();
    }

    fn apply(&mut self, from: usize, u: &[u8]) {
        if self.conns.get(&from).is_some_and(|c| c.view) {
            self.dropped_viewer_updates += 1;
            return;
        }
        let was_empty = self.empty();
        if let Ok(up) = Update::decode_v1(u) {
            let _ = self.doc.transact_mut().apply_update(up);
        }
        self.last_editor = Some(from);
        let others: Vec<usize> = self.conns.iter().filter(|(i, c)| **i != from && c.open && c.synced).map(|(i, _)| *i).collect();
        for o in others {
            self.send(o, &Frame::Update(u.to_vec()));
        }
        if was_empty && !self.empty() && !self.seeded {
            self.seeded = true;
            self.lease = None;
            let ids: Vec<usize> = self.conns.keys().copied().collect();
            for i in ids {
                self.control(i, json!({"type": "seeded", "modified": false}));
                let pending = self.conns.get_mut(&i).and_then(|c| {
                    let was = c.synced;
                    c.synced = true;
                    if was { None } else { Some(c.pending_sync1.take().unwrap_or_default()) }
                });
                if let Some(sv) = pending {
                    let d = self.diff(&sv);
                    self.send(i, &Frame::Sync2(d));
                    let sv = self.sv();
                    self.send(i, &Frame::Sync1(sv));
                }
            }
        }
    }

    fn on_frame(&mut self, from: usize, data: &[u8]) {
        let Some(f) = Frame::decode(data) else { panic!("the client sent a malformed frame: {data:?}") };
        self.got.entry(from).or_default().push(f.clone());
        let synced = self.conns.get(&from).is_some_and(|c| c.synced);
        match f {
            Frame::Control(t) => {
                let v: Json = serde_json::from_str(&t).unwrap();
                match v["type"].as_str().unwrap() {
                    "hello" => {
                        if let Some(c) = self.conns.get_mut(&from) {
                            c.client_id = v["clientId"].as_u64();
                        }
                        self.peers();
                    }
                    "saveNow" => self.ask_save(),
                    _ => {}
                }
            }
            Frame::Sync1(sv) => {
                if synced {
                    let d = self.diff(&sv);
                    self.send(from, &Frame::Sync2(d));
                } else if let Some(c) = self.conns.get_mut(&from) {
                    c.pending_sync1 = Some(sv);
                }
            }
            Frame::Sync2(u) | Frame::Update(u) => self.apply(from, &u),
            Frame::Awareness(a) => {
                let others: Vec<usize> = self.conns.iter().filter(|(i, c)| **i != from && c.open).map(|(i, _)| *i).collect();
                for o in others {
                    self.send(o, &Frame::Awareness(a.clone()));
                }
            }
            _ => {}
        }
    }

    fn ask_save(&mut self) {
        let Some(to) = self.last_editor else { return };
        let id = self.next_save;
        self.next_save += 1;
        self.control(to, json!({"type": "save", "id": id, "mode": "upload", "name": "Talk.pptx", "format": "pptx", "url": format!("https://live.test/api/v1/rooms/nc%3Acloud%3A42/file?save={id}"), "autosave": false}));
    }

    fn take(&mut self, who: usize) -> Vec<Vec<u8>> {
        self.conns.get_mut(&who).map(|c| std::mem::take(&mut c.out)).unwrap_or_default()
    }

    fn sent(&self, who: usize) -> &[Frame] {
        self.got.get(&who).map(Vec::as_slice).unwrap_or(&[])
    }
}

// ------------------------------------------------------------------ the peers

struct Peer {
    s: Session,
    c: LiveClient,
    who: usize,
    view: bool,
    connected: bool,
    http: Vec<HttpRequest>,
    posts: Vec<Json>,
    alerts: Vec<String>,
    statuses: Vec<String>,
    connects: usize,
}

impl Peer {
    fn new(who: usize) -> Peer {
        let mut s = Session::new();
        // The placeholder the web app shows while loading.
        s.execute("file.new", &json!({})).unwrap();
        Peer {
            s,
            c: LiveClient::new(),
            who,
            view: false,
            connected: false,
            http: vec![],
            posts: vec![],
            alerts: vec![],
            statuses: vec![],
            connects: 0,
        }
    }

    fn posted(&self, ty: &str) -> Vec<&Json> {
        self.posts.iter().filter(|p| p["type"] == ty).collect()
    }

    fn take_http(&mut self, method: &str) -> HttpRequest {
        let i = self
            .http
            .iter()
            .position(|r| r.method == method)
            .unwrap_or_else(|| panic!("no {method} request; have {:?}", self.http.iter().map(|r| (r.method, &r.url)).collect::<Vec<_>>()));
        self.http.remove(i)
    }
}

const NOW: u64 = 1_000_000;

struct World {
    room: Room,
    peers: Vec<Peer>,
    now: u64,
}

impl World {
    fn new(n: usize) -> World {
        World { room: Room::new(), peers: (0..n).map(Peer::new).collect(), now: NOW }
    }

    fn run(&mut self, i: usize, effects: Vec<Effect>) {
        for e in effects {
            let p = &mut self.peers[i];
            match e {
                Effect::Connect(url) => {
                    assert!(url.starts_with("wss://live.test/ws/nc%3Acloud%3A42?token="), "{url}");
                    p.connects += 1;
                    p.connected = true;
                    let (who, view) = (p.who, p.view);
                    self.room.connect(who, view);
                    let p = &mut self.peers[i];
                    let more = p.c.on_open(&mut p.s, self.now);
                    self.run(i, more);
                }
                Effect::Send(data) => {
                    assert!(self.peers[i].connected, "sent while disconnected");
                    self.room.on_frame(self.peers[i].who, &data);
                }
                Effect::Disconnect => {
                    p.connected = false;
                    let who = p.who;
                    self.room.disconnect(who);
                }
                Effect::Http(r) => p.http.push(r),
                Effect::Post(m) => p.posts.push(m),
                Effect::Status(t) => p.statuses.push(t),
                Effect::Alert(t) => p.alerts.push(t),
            }
        }
    }

    /// Delivers everything until nothing moves.
    fn settle(&mut self) {
        for _ in 0..50 {
            let mut moved = false;
            for i in 0..self.peers.len() {
                let who = self.peers[i].who;
                let frames = if self.peers[i].connected { self.room.take(who) } else { vec![] };
                for f in frames {
                    moved = true;
                    let p = &mut self.peers[i];
                    let e = p.c.on_frame(&mut p.s, &f, self.now);
                    self.run(i, e);
                }
                let p = &mut self.peers[i];
                let e = p.c.pump(&mut p.s, self.now);
                moved |= !e.is_empty();
                self.run(i, e);
            }
            if !moved {
                return;
            }
        }
        panic!("the world didn't settle");
    }

    fn start(&mut self, i: usize, seed: bool, view: bool) {
        self.peers[i].view = view;
        let user = json!({"id": format!("u{i}"), "name": format!("Person {i}"), "color": "#2E9E5B"});
        let frag = format!(
            "#craft-live=1&ws=wss%3A%2F%2Flive.test%2Fws%2Fnc%253Acloud%253A42&room=nc%3Acloud%3A42&token=tok{i}&user={}&role={}&seed={}&file=https%3A%2F%2Flive.test%2Fapi%2Fv1%2Frooms%2Fnc%253Acloud%253A42%2Ffile&name=Talk.pptx&format=pptx&api=https%3A%2F%2Flive.test%2Fapi%2Fv1&lang=en",
            pct_encode(&user.to_string()),
            if view { "view" } else { "edit" },
            if seed { 1 } else { 0 }
        );
        let cfg = Config::from_fragment(&frag).unwrap().unwrap();
        if seed {
            self.room.lease = Some(self.peers[i].who);
        }
        let p = &mut self.peers[i];
        let e = p.c.start(&mut p.s, cfg, self.now);
        self.run(i, e);
    }

    fn answer(&mut self, i: usize, req: &HttpRequest, status: u16, body: Vec<u8>) {
        let p = &mut self.peers[i];
        let e = p.c.on_http(&mut p.s, req.id, HttpResponse { status, body }, self.now);
        self.run(i, e);
    }

    fn exec(&mut self, i: usize, id: &str, params: Json) -> crate::Result<Json> {
        self.peers[i].s.execute(id, &params)
    }

    fn frame(&mut self, i: usize, data: &[u8], now: u64) -> Vec<Effect> {
        let p = &mut self.peers[i];
        p.c.on_frame(&mut p.s, data, now)
    }

    fn shell(&mut self, i: usize, msg: &Json, now: u64) -> Vec<Effect> {
        let p = &mut self.peers[i];
        p.c.on_shell(&mut p.s, msg, now)
    }

    fn closed(&mut self, i: usize, code: u16, reason: &str, now: u64) -> Vec<Effect> {
        let p = &mut self.peers[i];
        p.c.on_close(&mut p.s, code, reason, now)
    }

    fn fp(&self, i: usize) -> String {
        crate::collab::fingerprint(&self.peers[i].s.doc().unwrap().doc)
    }
}

/// A `.pptx` of three slides with a star on the second, made here.
fn talk_pptx() -> Vec<u8> {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    s.execute("shape.insert", &json!({"preset": "star5", "rect": [100, 100, 200, 200]})).unwrap();
    s.execute("slide.new", &json!({})).unwrap();
    crate::cmd::file::save_bytes(&s.doc().unwrap().doc, "pptx").unwrap()
}

fn slides(p: &Peer) -> usize {
    p.s.doc().unwrap().doc.slides.len()
}

/// One seeder, loaded and seeded, settled.
fn seeded_world(n: usize) -> World {
    let mut w = World::new(n);
    w.start(0, true, false);
    let get = w.peers[0].take_http("GET");
    assert_eq!(get.url, "https://live.test/api/v1/rooms/nc%3Acloud%3A42/file");
    assert!(get.headers.contains(&("Authorization".into(), "Bearer tok0".into())));
    w.answer(0, &get, 200, talk_pptx());
    w.settle();
    w
}

// ------------------------------------------------------------------ tests

#[test]
fn frames_round_trip_and_hostile_bytes_are_refused() {
    for f in [
        Frame::Sync1(vec![1, 2]),
        Frame::Sync2(vec![]),
        Frame::Update(vec![9; 300]),
        Frame::Awareness(vec![0]),
        Frame::Denied("no".into()),
        Frame::QueryAwareness,
        Frame::Control("{\"type\":\"hello\"}".into()),
    ] {
        assert_eq!(Frame::decode(&f.encode()), Some(f));
    }
    // The exact bytes of Appendix B and craft-live's control type (100 is the varuint 0x64).
    assert_eq!(Frame::Sync1(vec![7]).encode(), vec![0, 0, 1, 7]);
    assert_eq!(Frame::Update(vec![1, 2]).encode(), vec![0, 2, 2, 1, 2]);
    assert_eq!(Frame::Control("{}".into()).encode(), vec![100, 2, b'{', b'}']);
    let mut long = vec![0, 2];
    wire::write_var(&mut long, 300);
    assert_eq!(&long[2..], &[0xac, 0x02]);
    for bad in [&[][..], &[0, 2, 5, 1], &[0, 3, 0], &[9], &[0xff; 12], &[2, 1, 0], &[100, 2, 0xff, 0xfe]] {
        assert_eq!(Frame::decode(bad), None, "{bad:?}");
    }
}

#[test]
fn the_fragment_configures_the_client() {
    let f = "#craft-live=1&ws=wss%3A%2F%2Fl%2Fws%2Fnc%253Ax&room=nc%3Ax&token=abc%2B1&user=%7B%22id%22%3A%22ann%22%2C%22name%22%3A%22Ann%20Lee%22%2C%22color%22%3A%22%23D8553B%22%7D&role=view&seed=0&file=https%3A%2F%2Fl%2Ff&name=Budget+Q3.pptx&format=PPTX&api=https%3A%2F%2Fl%2Fapi%2Fv1%2F&lang=";
    let c = Config::from_fragment(f).unwrap().unwrap();
    assert_eq!(c.ws, "wss://l/ws/nc%3Ax");
    assert_eq!(c.room, "nc:x");
    assert_eq!(c.token, "abc+1");
    assert_eq!(c.user, Some(User { id: "ann".into(), name: "Ann Lee".into(), color: "#D8553B".into() }));
    assert!(c.view && !c.seed);
    assert_eq!((c.name.as_str(), c.format.as_str(), c.api.as_str(), c.title().as_str()), ("Budget Q3.pptx", "pptx", "https://l/api/v1", "Budget Q3"));
    assert_eq!(c.ws_url(), "wss://l/ws/nc%3Ax?token=abc%2B1");
    // Not craft-live's, another version, or unusable.
    assert!(Config::from_fragment("").is_none());
    assert!(Config::from_fragment("#blank").is_none());
    assert!(Config::from_fragment("craft-live=2&ws=wss://x").unwrap().is_err());
    assert!(Config::from_fragment("craft-live=1&ws=http://x&room=r&token=t").unwrap().is_err());
    assert!(Config::from_fragment("craft-live=1&ws=wss://x&room=r").unwrap().is_err());
    assert!(Config::from_fragment("craft-live=1&ws=wss://x&room=r&token=t&seed=1").unwrap().is_err(), "a seeder needs the file");
    // A bad colour becomes grey; a user without a name is no user.
    let c = Config::from_json(&json!({"ws": "wss://x", "room": "r", "token": "t", "user": {"id": "a", "name": "A", "color": "red"}, "seed": false}))
        .unwrap();
    assert_eq!(c.user.unwrap().color, "#808080");
    let c = Config::from_json(&json!({"ws": "wss://x", "room": "r", "token": "t", "user": {"id": "a"}})).unwrap();
    assert!(c.user.is_none());
    // Hostile fragments don't panic.
    for f in ["craft-live=1&%", "craft-live=1&ws=%zz&room=%&token=%e2%28", "&&&=&=craft-live=1", "craft-live=1&user=%7B"] {
        let _ = Config::from_fragment(f);
    }
    assert_eq!(pct_decode("a%2"), "a%2");
}

#[test]
fn saves_are_written_in_the_files_own_format_when_deckcraft_writes_it() {
    assert_eq!(save_target("pptx", "Talk.pptx"), ("pptx", "Talk.pptx".into()));
    assert_eq!(save_target("PPTM", "Talk.pptm"), ("pptm", "Talk.pptm".into()));
    assert_eq!(save_target("potx", "Model.potx"), ("potx", "Model.potx".into()));
    // .odp: DeckCraft doesn't write it, so a .pptx, which craft-live keeps as a copy.
    assert_eq!(save_target("odp", "Talk.odp"), ("pptx", "Talk.pptx".into()));
    assert_eq!(save_target("", ""), ("pptx", "Presentation.pptx".into()));
}

#[test]
fn the_first_editor_seeds_the_room_from_the_file_and_a_joiner_waits_for_it() {
    let mut w = World::new(2);
    w.start(0, true, false);
    // The joiner arrives while the file is loading: welcome {seeded: false}, held, Joining….
    w.start(1, false, false);
    w.settle();
    assert_eq!(crate::collab::read_only(&w.peers[1].s).as_deref(), Some("Joining…"));
    assert!(w.peers[1].posted("craft-live:loaded").is_empty(), "nothing is shown yet");
    assert_eq!(w.peers[1].s.documents().len(), 1, "the placeholder made way for the room's presentation");
    assert!(!w.room.seeded);

    let get = w.peers[0].take_http("GET");
    w.answer(0, &get, 200, talk_pptx());
    w.settle();
    assert!(w.room.seeded, "the seeder's sync step 2 filled the room");
    // The seeder: hello first (with its Yjs client id and the upload ability), then sync step 1.
    let sent = w.room.sent(0);
    let Frame::Control(hello) = &sent[0] else { panic!("{sent:?}") };
    let hello: Json = serde_json::from_str(hello).unwrap();
    assert_eq!(hello["type"], "hello");
    assert_eq!(hello["can"], json!(["upload"]));
    assert_eq!(hello["clientId"].as_u64(), Some(w.peers[0].s.active_room().unwrap().client_id()));
    assert!(matches!(sent[1], Frame::Sync1(_)), "{sent:?}");
    assert!(sent.iter().any(|f| matches!(f, Frame::Sync2(u) if u.len() > 100)), "the whole presentation went in sync step 2");
    // Both show the file's three slides, the same presentation.
    assert_eq!(slides(&w.peers[0]), 3);
    assert_eq!(slides(&w.peers[1]), 3);
    assert_eq!(w.fp(0), w.fp(1));
    assert_eq!(crate::collab::read_only(&w.peers[1].s), None);
    for p in &w.peers {
        assert_eq!(p.posted("craft-live:loaded").len(), 1);
        assert_eq!(p.s.documents().len(), 1);
        assert_eq!(p.s.doc().unwrap().title(), "Talk");
        assert!(!p.s.doc().unwrap().is_dirty());
    }
    assert_eq!(w.peers[1].posted("craft-live:modified").last().unwrap()["modified"], false);
    // Peers: the others and yourself.
    let peers = &w.peers[0].posted("craft-live:peers").last().unwrap()["peers"];
    assert_eq!(peers[0]["self"], true);
    assert_eq!(peers[0]["user"]["name"], "Person 0");
    assert!(peers.as_array().unwrap().iter().any(|p| p["user"]["name"] == "Person 1"));
    // The room's people reach the engine (the popover's list).
    assert_eq!(w.peers[0].s.active_room().unwrap().listed.len(), 1);
}

#[test]
fn every_committed_command_is_one_update_and_reaches_the_others() {
    let mut w = seeded_world(2);
    w.start(1, false, false);
    w.settle();
    assert_eq!(w.fp(0), w.fp(1));
    let before = w.room.sent(0).iter().filter(|f| matches!(f, Frame::Update(_))).count();
    w.exec(0, "slide.new", json!({})).unwrap();
    w.settle();
    let after = w.room.sent(0).iter().filter(|f| matches!(f, Frame::Update(_))).count();
    assert_eq!(after - before, 1, "one command, one update");
    assert_eq!(slides(&w.peers[1]), 4);
    w.exec(1, "shape.insert", json!({"preset": "rect", "rect": [10, 10, 50, 50]})).unwrap();
    w.exec(1, "chat.send", json!({"text": "Looks good"})).unwrap();
    w.settle();
    assert_eq!(w.fp(0), w.fp(1));
    assert_eq!(w.peers[0].s.doc().unwrap().doc.chat.messages.len(), 1);
    assert_eq!(w.peers[0].s.doc().unwrap().doc.chat.messages[0].author.name, "Person 1", "the chat's author is Nextcloud's person");
    // Awareness travels too, and shows the others' selections.
    w.now += 1_000;
    w.exec(1, "slide.go", json!({"index": 1})).unwrap();
    w.settle();
    assert!(w.room.sent(1).iter().any(|f| matches!(f, Frame::Awareness(_))));
    let peers = w.peers[0].s.active_room().unwrap().peers();
    assert!(peers.iter().any(|p| p.user.name == "Person 1"), "{peers:?}");
}

#[test]
fn a_save_uploads_the_presentation_in_the_files_format() {
    let mut w = seeded_world(1);
    w.exec(0, "slide.new", json!({})).unwrap();
    w.settle();
    // Nextcloud's Action_Save reaches the app as craft-live:save; Save in the app sends saveNow.
    let p = &mut w.peers[0];
    let e = p.c.on_shell(&mut p.s, &json!({"type": "craft-live:save", "notify": true}), w.now);
    w.run(0, e);
    let p = &mut w.peers[0];
    let e = p.c.save_now(false);
    w.run(0, e);
    w.settle();
    let put = w.peers[0].take_http("PUT");
    assert_eq!(put.url, "https://live.test/api/v1/rooms/nc%3Acloud%3A42/file?save=1");
    assert!(put.headers.contains(&("Authorization".into(), "Bearer tok0".into())));
    assert!(put.headers.contains(&("X-Craft-File-Name".into(), "Talk.pptx".into())));
    let body = put.body.clone().unwrap();
    let mut check = Session::new();
    check.execute("file.openBytes", &json!({"name": "Talk.pptx", "data": crate::cmd::base64_encode(&body)})).unwrap();
    assert_eq!(check.doc().unwrap().doc.slides.len(), 4, "the uploaded file holds the room's presentation");
    w.answer(0, &put, 200, br#"{"saved":true,"version":"7"}"#.to_vec());
    // The room says it's saved: craft-live:saved and modified false for the shell.
    let saved = Frame::control(&json!({"type": "saved", "at": 1, "modified": false})).encode();
    let p = &mut w.peers[0];
    let e = p.c.on_frame(&mut p.s, &saved, w.now);
    w.run(0, e);
    assert_eq!(w.peers[0].posted("craft-live:saved").len(), 1);
    assert_eq!(w.peers[0].posted("craft-live:saved")[0]["success"], true);
    assert_eq!(w.peers[0].posted("craft-live:modified").last().unwrap()["modified"], false);
    assert!(!w.peers[0].s.doc().unwrap().is_dirty(), "saved into Nextcloud: no longer Edited");
    // An autosave's `saved` posts no craft-live:saved.
    let e = w.frame(0, &saved, w.now);
    w.run(0, e);
    assert_eq!(w.peers[0].posted("craft-live:saved").len(), 1);

    // .odp: DeckCraft writes a .pptx under a .pptx name (craft-live keeps it as a copy).
    let ask =
        Frame::control(&json!({"type": "save", "id": 9, "mode": "upload", "name": "Talk.odp", "format": "odp", "url": "https://live.test/u?save=9"}))
            .encode();
    let e = w.frame(0, &ask, w.now);
    w.run(0, e);
    let put = w.peers[0].take_http("PUT");
    assert!(put.headers.contains(&("X-Craft-File-Name".into(), "Talk.pptx".into())));
    // A failed upload is reported to the room.
    w.answer(0, &put, 409, br#"{"error":"no such save is waiting for this client"}"#.to_vec());
    let last = w.room.sent(0).last().cloned().unwrap();
    let Frame::Control(t) = last else { panic!() };
    let v: Json = serde_json::from_str(&t).unwrap();
    assert_eq!((v["type"].as_str(), v["id"].as_u64(), v["ok"].as_bool()), (Some("saveResult"), Some(9), Some(false)));
    assert!(v["error"].as_str().unwrap().contains("no such save"));
    // WebDAV mode isn't for the web app.
    let ask = Frame::control(&json!({"type": "save", "id": 10, "mode": "webdav", "name": "Talk.pptx", "format": "pptx", "etag": "x"})).encode();
    let e = w.frame(0, &ask, w.now);
    w.run(0, e);
    assert!(w.peers[0].http.is_empty());
    let Frame::Control(t) = w.room.sent(0).last().cloned().unwrap() else { panic!() };
    assert!(t.contains("\"saveResult\"") && t.contains("\"id\":10"));
    // A failed explicit save.
    let e = w.peers[0].c.save_now(false);
    w.run(0, e);
    w.settle();
    let put = w.peers[0].take_http("PUT");
    w.answer(0, &put, 502, vec![]);
    let failed = Frame::control(&json!({"type": "saveFailed", "message": "Nextcloud refused the file"})).encode();
    let e = w.frame(0, &failed, w.now);
    w.run(0, e);
    assert_eq!(w.peers[0].posted("craft-live:saved").last().unwrap()["success"], false);
    assert!(w.peers[0].statuses.last().unwrap().contains("Nextcloud refused the file"));
}

#[test]
fn control_messages_reach_the_shell_and_the_person() {
    let mut w = seeded_world(1);
    let feed = |w: &mut World, v: Json| {
        let f = Frame::control(&v).encode();
        let e = w.frame(0, &f, w.now);
        w.run(0, e);
    };
    feed(&mut w, json!({"type": "modified", "modified": true}));
    assert_eq!(w.peers[0].posted("craft-live:modified").last().unwrap()["modified"], true);
    assert!(w.peers[0].c.modified);
    feed(&mut w, json!({"type": "notice", "level": "warning", "text": "The file changed in Nextcloud: saved as a copy"}));
    assert_eq!(w.peers[0].alerts.last().unwrap(), "The file changed in Nextcloud: saved as a copy");
    feed(&mut w, json!({"type": "notice", "level": "info", "text": "Saved"}));
    assert_eq!(w.peers[0].statuses.last().unwrap(), "Saved");
    feed(&mut w, json!({"type": "fly", "x": 1}));
    feed(&mut w, json!({"type": 7}));
    // The shell asks for the peers.
    let e = w.shell(0, &json!({"type": "craft-live:getPeers"}), w.now);
    w.run(0, e);
    assert_eq!(w.peers[0].posted("craft-live:peers").last().unwrap()["peers"][0]["self"], true);
    // Messages it doesn't know (craft-live:host…) are ignored.
    let e = w.shell(0, &json!({"type": "craft-live:host", "message": "Hide_Menu_Item", "values": {}}), w.now);
    assert!(e.is_empty());
    // Expired: the person is told to reopen; no reconnect.
    feed(&mut w, json!({"type": "expired"}));
    let e = w.closed(0, close::EXPIRED, "", w.now);
    w.run(0, e);
    assert_eq!(w.peers[0].c.phase, Phase::Expired);
    assert!(w.peers[0].alerts.last().unwrap().contains("open it again"));
    assert!(w.peers[0].posted("craft-live:error").is_empty(), "the document was loaded");
    w.now += 120_000;
    w.settle();
    assert_eq!(w.peers[0].connects, 1, "an expired session doesn't reconnect");
}

#[test]
fn a_dropped_connection_reconnects_with_backoff_and_merges_the_edits() {
    let mut w = seeded_world(2);
    w.start(1, false, false);
    w.settle();
    // Peer 1 loses its connection; both go on editing.
    w.peers[1].connected = false;
    w.room.disconnect(1);
    let e = w.closed(1, 1006, "", w.now);
    w.run(1, e);
    assert_eq!(w.peers[1].c.phase, Phase::Offline { retry_at: w.now + RETRY_MIN_MS });
    w.settle();
    assert_eq!(w.peers[1].s.active_room().unwrap().state, RoomState::Offline);
    w.exec(1, "slide.new", json!({})).unwrap();
    w.exec(0, "shape.insert", json!({"preset": "ellipse", "rect": [0, 0, 40, 40]})).unwrap();
    w.settle();
    assert_ne!(w.fp(0), w.fp(1));
    assert_eq!(w.peers[1].connects, 1, "not before the backoff");
    // A failed attempt doubles the wait.
    w.now += RETRY_MIN_MS;
    let p = &mut w.peers[1];
    let e = p.c.pump(&mut p.s, w.now);
    assert!(e.iter().any(|e| matches!(e, Effect::Connect(_))));
    let e = p.c.on_close(&mut p.s, 1006, "", w.now);
    w.run(1, e);
    assert_eq!(w.peers[1].c.phase, Phase::Offline { retry_at: w.now + 2 * RETRY_MIN_MS });
    w.now += 2 * RETRY_MIN_MS;
    w.settle();
    assert_eq!(w.peers[1].connects, 2);
    assert_eq!(w.fp(0), w.fp(1), "both sides' edits merged after the reconnect");
    assert_eq!(slides(&w.peers[0]), 4);
    assert_eq!(w.peers[1].s.active_room().unwrap().state, RoomState::Online);
    // The second hello came first on the new connection, then sync step 1.
    let sent = w.room.sent(1);
    let hellos: Vec<usize> =
        sent.iter().enumerate().filter(|(_, f)| matches!(f, Frame::Control(t) if t.contains("\"hello\""))).map(|(i, _)| i).collect();
    assert_eq!(hellos.len(), 2);
    assert!(matches!(sent.get(hellos[1] + 1), Some(Frame::Sync1(_))), "{sent:?}");
    // 4408 (too slow) reconnects too; 4403 doesn't.
    let e = w.closed(1, close::TOO_SLOW, "", w.now);
    assert!(e.iter().all(|e| !matches!(e, Effect::Alert(_))));
    assert!(matches!(w.peers[1].c.phase, Phase::Offline { .. }));
    let deny = Frame::Denied("no edit rights".into()).encode();
    w.now += 60_000;
    w.settle();
    let e = w.frame(1, &deny, w.now);
    w.run(1, e);
    let e = w.closed(1, close::REFUSED, "", w.now);
    w.run(1, e);
    assert_eq!(w.peers[1].c.phase, Phase::Refused("no edit rights".into()));
}

#[test]
fn when_the_seeder_leaves_early_a_joiner_asks_for_the_lease_and_seeds() {
    let mut w = World::new(2);
    w.start(0, true, false);
    w.start(1, false, false);
    w.settle();
    // The seeder never finishes loading and goes.
    let _ = w.peers[0].take_http("GET");
    w.room.disconnect(0);
    w.peers[0].connected = false;
    w.settle();
    // The joiner got `reseed`: it asks for the lease again.
    let prep = w.peers[1].take_http("POST");
    assert_eq!(prep.url, "https://live.test/api/v1/prepare");
    assert!(prep.headers.contains(&("Authorization".into(), "Bearer tok1".into())));
    assert!(w.peers[1].s.active_room().is_none(), "it left the room it was waiting in");
    w.room.lease = Some(1);
    w.answer(1, &prep, 200, br#"{"seed":true,"seeded":false}"#.to_vec());
    let get = w.peers[1].take_http("GET");
    w.answer(1, &get, 200, talk_pptx());
    w.settle();
    assert!(w.room.seeded);
    assert_eq!(slides(&w.peers[1]), 3);
    assert_eq!(w.peers[1].s.documents().len(), 1);
    assert_eq!(w.peers[1].connects, 2);
    assert!(w.peers[1].posted("craft-live:close").is_empty(), "a reseed isn't a Close");
    assert_eq!(w.peers[1].posted("craft-live:loaded").len(), 1);

    // Without the lease: it joins and waits.
    let mut w = World::new(2);
    w.start(0, true, false);
    w.start(1, false, false);
    w.settle();
    w.room.disconnect(0);
    w.peers[0].connected = false;
    w.settle();
    let prep = w.peers[1].take_http("POST");
    w.answer(1, &prep, 200, br#"{"seed":false,"seeded":false}"#.to_vec());
    w.settle();
    assert_eq!(crate::collab::read_only(&w.peers[1].s).as_deref(), Some("Joining…"));
}

#[test]
fn a_seeder_whose_lease_went_elsewhere_never_sends_its_copy() {
    let mut w = World::new(1);
    w.start(0, true, false);
    w.room.lease = None; // expired before the client connected
    let get = w.peers[0].take_http("GET");
    w.answer(0, &get, 200, talk_pptx());
    w.settle();
    assert!(!w.room.sent(0).iter().any(|f| matches!(f, Frame::Sync2(_) | Frame::Update(_) | Frame::Sync1(_))), "{:?}", w.room.sent(0));
    assert!(w.room.empty());
    assert_eq!(w.peers[0].c.phase, Phase::Preparing);
    w.peers[0].take_http("POST");
}

#[test]
fn viewers_see_the_presentation_read_only_and_send_no_edits() {
    let mut w = seeded_world(2);
    w.start(1, false, true);
    w.settle();
    assert_eq!(w.fp(0), w.fp(1));
    assert!(crate::collab::read_only(&w.peers[1].s).unwrap().contains("view"));
    let _ = w.exec(1, "slide.new", json!({}));
    w.settle();
    assert!(!w.room.sent(1).iter().any(|f| matches!(f, Frame::Update(_))), "no update was even sent");
    assert_eq!(w.fp(0), w.fp(1));
    assert_eq!(slides(&w.peers[1]), 3);
    // Saving isn't theirs.
    let e = w.peers[1].c.save_now(false);
    assert!(matches!(&e[..], [Effect::Status(_)]));
    // A viewer never seeds, even when told to.
    let mut v = World::new(1);
    v.start(0, true, true);
    assert!(v.peers[0].http.is_empty());
}

#[test]
fn closing_leaves_the_room_and_tells_the_shell() {
    let mut w = seeded_world(1);
    let p = &mut w.peers[0];
    let e = p.c.close(&mut p.s);
    w.run(0, e);
    assert_eq!(w.peers[0].posted("craft-live:close").len(), 1);
    assert!(matches!(w.room.sent(0).last(), Some(Frame::Control(t)) if t.contains("bye")));
    assert!(!w.peers[0].connected);
    assert!(w.peers[0].s.active_room().is_none());
    // File › Close (or the session's Leave) does the same.
    let mut w = seeded_world(1);
    w.exec(0, "file.close", json!({})).unwrap();
    w.settle();
    assert_eq!(w.peers[0].posted("craft-live:close").len(), 1);
    assert!(!w.peers[0].connected);
    // The shell closing the editor (Nextcloud's Close) leaves without echoing craft-live:close.
    let mut w = seeded_world(1);
    let e = w.shell(0, &json!({"type": "craft-live:close"}), w.now);
    w.run(0, e);
    assert!(w.peers[0].posted("craft-live:close").is_empty());
    assert_eq!(w.peers[0].c.phase, Phase::Closed);
}

#[test]
fn failures_before_the_presentation_shows_are_reported_to_the_shell() {
    let mut w = World::new(1);
    w.start(0, true, false);
    let get = w.peers[0].take_http("GET");
    w.answer(0, &get, 401, br#"{"error":"Nextcloud refused the session"}"#.to_vec());
    let err = w.peers[0].posted("craft-live:error");
    assert_eq!(err.len(), 1);
    assert!(err[0]["message"].as_str().unwrap().contains("Nextcloud refused the session"));
    // A file DeckCraft can't read.
    let mut w = World::new(1);
    w.start(0, true, false);
    let get = w.peers[0].take_http("GET");
    w.answer(0, &get, 200, b"not a presentation at all".to_vec());
    assert_eq!(w.peers[0].posted("craft-live:error").len(), 1);
    assert!(matches!(w.peers[0].c.phase, Phase::Failed(_)));
    // A 0-byte file is a new, empty presentation.
    let mut w = World::new(1);
    w.start(0, true, false);
    let get = w.peers[0].take_http("GET");
    w.answer(0, &get, 200, vec![]);
    w.settle();
    assert!(w.room.seeded);
    assert_eq!(w.peers[0].posted("craft-live:loaded").len(), 1);
    // Bad configuration from the shell.
    let mut w = World::new(1);
    let e = w.shell(0, &json!({"type": "craft-live:config", "config": {"room": "r"}}), w.now);
    w.run(0, e);
    assert_eq!(w.peers[0].posted("craft-live:error").len(), 1);
}

#[test]
fn the_shells_config_message_starts_the_client() {
    let mut w = World::new(1);
    let cfg = json!({"type": "craft-live:config", "config": {"v": 1, "ws": "wss://live.test/ws/nc%3Acloud%3A42", "room": "nc:cloud:42", "token": "tok0", "user": {"id": "u0", "name": "Person 0", "color": "#2E9E5B"}, "role": "edit", "seed": false, "file": "https://live.test/f", "name": "Talk.pptx", "format": "pptx", "api": "https://live.test/api/v1", "lang": ""}});
    let e = w.shell(0, &cfg, w.now);
    w.run(0, e);
    assert_eq!(w.peers[0].connects, 1);
    // A second config is ignored.
    let e = w.shell(0, &cfg, w.now);
    assert!(e.is_empty());
    assert_eq!(LiveClient::hello(), Effect::Post(json!({"type": "craft-live:hello"})));
}

#[test]
fn hostile_frames_never_crash_the_client() {
    let mut w = seeded_world(1);
    let mut rng = deckcraft_collab::Rng::seeded(7);
    for i in 0..2_000u64 {
        let len = rng.range(0, 64) as usize;
        let mut data: Vec<u8> = (0..len).map(|_| rng.next_u64() as u8).collect();
        // Mostly well-framed, so the payloads reach the engine and the control parser.
        let head: Vec<u8> = match i % 4 {
            0 => vec![0, rng.range(0, 3) as u8, len as u8],
            1 => vec![1, len as u8],
            2 => vec![100, len as u8],
            _ => vec![],
        };
        data.splice(0..0, head);
        let e = w.frame(0, &data, w.now);
        w.run(0, e);
    }
    for v in [
        json!({"type": "save"}),
        json!({"type": "save", "id": "x", "mode": "upload", "url": 5}),
        json!({"type": "welcome", "seeded": "yes"}),
        json!({"type": "peers", "peers": 3}),
    ] {
        let f = Frame::control(&v).encode();
        let e = w.frame(0, &f, w.now);
        w.run(0, e);
    }
    w.settle();
    assert_eq!(slides(&w.peers[0]), 3, "the presentation survived");
}

#[test]
fn the_desktop_snapshot_takes_a_format_hint() {
    let mut s = Session::with_new();
    s.collab_serve_in_memory(TRANSPORT);
    s.execute("collab.join", &json!({"plugin": TRANSPORT, "room": "r", "mode": "share", "title": "Model"})).unwrap();
    let ask = |s: &mut Session, id: u64, format: Option<&str>| {
        let mut params = json!({"room": "r", "file": true});
        if let Some(f) = format {
            params["format"] = json!(f);
        }
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": "collab/snapshot", "params": params});
        s.collab_poll_pushed(1, vec![Pushed { plugin: TRANSPORT.into(), incoming: Incoming::Message(msg) }]);
        s.collab_take_outgoing().into_iter().find(|m| m["id"] == id).unwrap()["result"].clone()
    };
    let r = ask(&mut s, 1, Some("potx"));
    assert_eq!((r["name"].as_str(), r["format"].as_str()), (Some("Presentation1.potx"), Some("potx")));
    let bytes = deckcraft_collab::b64_decode(r["file"].as_str().unwrap()).unwrap();
    let mut check = Session::new();
    check.execute("file.openBytes", &json!({"name": "x.potx", "data": crate::cmd::base64_encode(&bytes)})).unwrap();
    let r = ask(&mut s, 2, None);
    assert_eq!(r["format"], "pptx");
    let r = ask(&mut s, 3, Some("odp"));
    assert_eq!(r["format"], "pptx", "a format DeckCraft doesn't write gives a .pptx");
}
