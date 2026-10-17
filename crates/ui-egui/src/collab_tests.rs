//! Live sessions in the window: the Share button at the right end of the title bar, its state in
//! a session, the people's circles beside it (you and the others, from their awareness), the
//! session popover with Leave, and the Share window listing the transport add-ins.

use deckcraft_engine::Session;
use deckcraft_engine::collab::IN_MEMORY;
use deckcraft_engine::collab_core::{awareness, b64_encode, id_str};
use deckcraft_engine::plugin::push::{Incoming, Pushed};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value as Json, json};

use crate::{Services, SlideApp};

fn harness(s: Session) -> Harness<'static, SlideApp> {
    let app = SlideApp::new(s, Services::default());
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

/// A message from the (in-memory) transport to the session.
fn deliver(h: &mut Harness<'static, SlideApp>, msg: Json) {
    let now = h.state().session.collab.now() + 100;
    h.state_mut().session.collab_poll_pushed(now, vec![Pushed { plugin: IN_MEMORY.into(), incoming: Incoming::Message(msg) }]);
    h.run_steps(2);
}

#[test]
fn share_button_people_circles_and_the_session_popover() {
    let mut h = harness(Session::with_new());
    assert!(h.query_by_label("Share").is_some(), "the Share button at the right of the title bar");
    // A session through a transport the test plays (in memory).
    h.state_mut()
        .session
        .collab_join_in_memory(json!({"room": "ui:room", "mode": "share", "user": {"id": "u-ann", "name": "Ann Lee", "color": "#3B7DD8"}}))
        .unwrap();
    h.run_steps(2);
    assert!(h.query_by_label("Connecting…").is_some(), "the button shows the session's state");
    assert!(h.query_by_label("Ann Lee (you)").is_some(), "your own circle");
    deliver(&mut h, json!({"jsonrpc": "2.0", "method": "collab/status", "params": {"room": "ui:room", "state": "online", "peers": []}}));
    assert!(h.query_by_label("Live").is_some());
    // Bob arrives (his awareness) with a shape selected on the first slide.
    let (slide, shape) = {
        let d = h.state().session.active().unwrap();
        let s = d.doc.slides.first().unwrap();
        (s.id.0, s.shapes.first().unwrap().id.0)
    };
    let state = json!({"user": {"id": "u-bob", "name": "Bob", "color": "#D8513B"}, "selection": {"slide": id_str(slide), "shapes": [id_str(shape)]}});
    let update = awareness::encode(&[(4242, 1, Some(&state))]);
    deliver(
        &mut h,
        json!({"jsonrpc": "2.0", "method": "collab/awareness", "params": {"room": "ui:room", "update": b64_encode(&update), "from": 4242}}),
    );
    assert!(h.query_by_label("Bob").is_some(), "Bob's circle");
    // Clicking Bob goes to his selection.
    h.get_by_label("Bob").click();
    h.run_steps(2);
    assert_eq!(h.state().session.active().unwrap().selection.shapes.first().map(|s| s.0), Some(shape));
    // More than four people: four circles and "+N".
    for (i, name) in ["Cy", "Di", "Ed", "Flo"].iter().enumerate() {
        let st = json!({"user": {"id": format!("u-{name}"), "name": name, "color": "#2E9E5B"}});
        let up = awareness::encode(&[(5000 + i as u64, 1, Some(&st))]);
        deliver(&mut h, json!({"jsonrpc": "2.0", "method": "collab/awareness", "params": {"room": "ui:room", "update": b64_encode(&up)}}));
    }
    assert!(h.query_by_label("+2").is_some(), "six people: four circles and +2");
    // The session popover: who's here, Leave.
    h.get_by_label("Live").click();
    h.run_steps(2);
    assert!(h.state().collab.popover);
    h.get_by_label("Leave").click();
    h.run_steps(3);
    assert!(h.state().session.active_room().is_none(), "left");
    assert!(h.query_by_label("Share").is_some(), "back to Share");
    assert!(h.query_by_label("Bob").is_none());
}

#[test]
fn share_opens_the_share_window_with_the_transports() {
    let m = deckcraft_engine::plugin::Manifest::parse(
        &json!({
            "id": "t.ui-transport", "name": "Test transport", "run": {"command": "./not-started"},
            "commands": [{"id": "share", "label": "Share"}, {"id": "join", "label": "Join"}],
            "collab": {"transport": "test", "label": "Live (Test)"}
        })
        .to_string(),
    )
    .unwrap();
    deckcraft_engine::plugin::install(m, std::env::temp_dir()).unwrap();
    struct Unload;
    impl Drop for Unload {
        fn drop(&mut self) {
            deckcraft_engine::plugin::unload("t.ui-transport");
        }
    }
    let _unload = Unload;
    let mut h = harness(Session::with_new());
    h.get_by_label("Share").click();
    h.run_steps(3);
    let w = h.state().collab.share.clone().expect("the Share window");
    assert_eq!(w.plugin, "t.ui-transport");
    assert!(h.query_by_label("Start live session").is_some());
    assert!(h.query_by_label("Join a session…").is_some());
    // `collab.share` with no params (a ribbon or palette entry) opens it too.
    h.state_mut().collab.share = None;
    h.state_mut().run("collab.share", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.state().collab.share.is_some());
}

#[test]
fn embedded_in_craft_live_save_and_close_go_to_the_page_around_the_app() {
    let mut h = harness(Session::with_new());
    assert!(h.query_by_label("Close").is_none(), "no Close button outside an embedding page");
    h.state_mut().embed = Some(crate::Embed { host: "Nextcloud".into(), ..Default::default() });
    h.run_steps(2);
    // Save (Cmd+S, the quick access toolbar, File › Save) asks the page to save.
    h.state_mut().save();
    assert!(h.state().embed.as_ref().unwrap().save_requested);
    // The title bar's Close.
    h.get_by_label("Close").click();
    h.run_steps(2);
    assert!(h.state().embed.as_ref().unwrap().close_requested);
    h.state_mut().embed.as_mut().unwrap().close_requested = false;
    // In the room, the session popover's Close (in place of Leave) closes the editor too, and
    // doesn't leave the room by itself.
    h.state_mut().session.collab_serve_in_memory(deckcraft_engine::live::TRANSPORT);
    h.state_mut()
        .session
        .execute("collab.join", &json!({"plugin": deckcraft_engine::live::TRANSPORT, "room": "nc:cloud:42", "mode": "share", "user": {"id": "ann", "name": "Ann Lee", "color": "#3B7DD8"}}))
        .unwrap();
    h.run_steps(2);
    h.get_by_label("Connecting…").click();
    h.run_steps(2);
    assert!(h.query_by_label("Leave").is_none());
    let closes = h.get_all_by_label("Close").count();
    assert_eq!(closes, 2, "the title bar's and the popover's");
    h.get_all_by_label("Close").last().unwrap().click();
    h.run_steps(2);
    assert!(h.state().embed.as_ref().unwrap().close_requested);
    assert!(h.state().session.active_room().is_some(), "the host leaves the room as it closes");
}
