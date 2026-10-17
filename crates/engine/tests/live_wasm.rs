//! The craft-live client in WebAssembly, where the web build runs it: configuring from the
//! fragment, loading and seeding a `.pptx`, a second client joining from the room's state, an
//! edit reaching it as one update, the chat, and a save uploading the file. Native builds run the
//! full suite in `src/live_tests.rs`; this checks the same path compiles and behaves in wasm32
//! (randomness, clocks, the `.pptx` reader and writer).
//!
//! Run: `cargo test -p deckcraft-engine --target wasm32-unknown-unknown --test live_wasm` with
//! `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner` (Node 16 needs
//! `NODE_ARGS=--experimental-wasm-reftypes`).
#![cfg(target_arch = "wasm32")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use deckcraft_engine::Session;
use deckcraft_engine::live::wire::Frame;
use deckcraft_engine::live::{Config, Effect, HttpResponse, LiveClient};
use serde_json::json;
use wasm_bindgen_test::wasm_bindgen_test;

fn frag(seed: bool, token: &str) -> Config {
    let f = format!(
        "#craft-live=1&ws=wss%3A%2F%2Flive.test%2Fws%2Fr&room=r&token={token}&user=%7B%22id%22%3A%22{token}%22%2C%22name%22%3A%22{token}%22%2C%22color%22%3A%22%233B7DD8%22%7D&role=edit&seed={}&file=https%3A%2F%2Flive.test%2Ff&name=Talk.pptx&format=pptx&api=https%3A%2F%2Flive.test%2Fapi%2Fv1",
        if seed { 1 } else { 0 }
    );
    Config::from_fragment(&f).unwrap().unwrap()
}

fn sends(e: &[Effect]) -> Vec<Frame> {
    e.iter().filter_map(|e| if let Effect::Send(d) = e { Frame::decode(d) } else { None }).collect()
}

fn feed(c: &mut LiveClient, s: &mut Session, f: Frame) -> Vec<Effect> {
    let mut out = c.on_frame(s, &f.encode(), 1);
    out.extend(c.pump(s, 1));
    out
}

#[wasm_bindgen_test]
fn seed_join_edit_and_save_in_wasm() {
    // The file: a .pptx of two slides, made here.
    let mut maker = Session::with_new();
    maker.execute("slide.new", &json!({})).unwrap();
    let pptx = deckcraft_engine::cmd::file::save_bytes(&maker.doc().unwrap().doc, "pptx").unwrap();

    // The seeder loads it and answers the room's sync step 1 with the whole presentation.
    let (mut a, mut sa) = (LiveClient::new(), Session::new());
    let e = a.start(&mut sa, frag(true, "a"), 1);
    let Some(Effect::Http(get)) = e.iter().find(|e| matches!(e, Effect::Http(_))).cloned() else { panic!("{e:?}") };
    let e = a.on_http(&mut sa, get.id, HttpResponse { status: 200, body: pptx }, 1);
    assert!(e.iter().any(|e| matches!(e, Effect::Connect(u) if u == "wss://live.test/ws/r?token=a")));
    let hello = sends(&a.on_open(&mut sa, 1));
    assert!(matches!(&hello[..], [Frame::Control(t)] if t.contains("\"hello\"")));
    a.pump(&mut sa, 1);
    feed(&mut a, &mut sa, Frame::control(&json!({"type": "welcome", "seed": true, "seeded": false})));
    let e = feed(&mut a, &mut sa, Frame::Sync1(vec![0]));
    let seed = sends(&e).into_iter().find_map(|f| if let Frame::Sync2(u) = f { Some(u) } else { None }).unwrap();
    assert!(seed.len() > 100);
    assert_eq!(sa.doc().unwrap().doc.slides.len(), 2);

    // A joiner gets the room's state as sync step 2 and shows it.
    let (mut b, mut sb) = (LiveClient::new(), Session::new());
    b.start(&mut sb, frag(false, "b"), 1);
    b.on_open(&mut sb, 1);
    b.pump(&mut sb, 1);
    feed(&mut b, &mut sb, Frame::control(&json!({"type": "welcome", "seed": false, "seeded": true})));
    let e = feed(&mut b, &mut sb, Frame::Sync2(seed));
    assert!(e.iter().any(|e| matches!(e, Effect::Post(m) if m["type"] == "craft-live:loaded")));
    assert_eq!(sb.doc().unwrap().doc.slides.len(), 2);

    // An edit and a chat message: one update each, applied on the other side.
    sa.execute("slide.new", &json!({})).unwrap();
    sa.execute("chat.send", &json!({"text": "from wasm"})).unwrap();
    let ups: Vec<Vec<u8>> = sends(&a.pump(&mut sa, 2)).into_iter().filter_map(|f| if let Frame::Update(u) = f { Some(u) } else { None }).collect();
    assert_eq!(ups.len(), 2);
    for u in ups {
        feed(&mut b, &mut sb, Frame::Update(u));
    }
    assert_eq!(sb.doc().unwrap().doc.slides.len(), 3);
    assert_eq!(sb.doc().unwrap().doc.chat.messages.first().map(|m| m.text.as_str()), Some("from wasm"));
    assert_eq!(deckcraft_engine::collab::fingerprint(&sa.doc().unwrap().doc), deckcraft_engine::collab::fingerprint(&sb.doc().unwrap().doc));

    // The room asks the joiner for the file: a .pptx upload with the bearer token.
    let e = feed(
        &mut b,
        &mut sb,
        Frame::control(
            &json!({"type": "save", "id": 1, "mode": "upload", "name": "Talk.pptx", "format": "pptx", "url": "https://live.test/u?save=1"}),
        ),
    );
    let Some(Effect::Http(put)) = e.into_iter().find(|e| matches!(e, Effect::Http(_))) else { panic!() };
    assert_eq!(put.method, "PUT");
    assert!(put.headers.contains(&("Authorization".into(), "Bearer b".into())));
    let mut check = Session::new();
    check.execute("file.openBytes", &json!({"name": "Talk.pptx", "data": deckcraft_engine::cmd::base64_encode(&put.body.unwrap())})).unwrap();
    assert_eq!(check.doc().unwrap().doc.slides.len(), 3);
}
