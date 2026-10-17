//! The document chat in the window: the title bar's chat button (shown in a session, and outside
//! one when the presentation has a chat) with the unread count and the mention flash, the Chat
//! pane (messages, "Message deleted", Enter sends and Shift+Enter doesn't, who's typing), its menu
//! with the Office note and Clear chat… asking first.

use std::sync::Arc;

use deckcraft_engine::Session;
use deckcraft_engine::collab::IN_MEMORY;
use deckcraft_engine::plugin::push::{Incoming, Pushed};
use deckcraft_model::chat::{ChatAuthor, ChatMessage};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

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

/// A message from someone else, as a file or a peer brings it.
fn from_bob(h: &mut Harness<'static, SlideApp>, id: &str, text: &str) {
    let st = h.state_mut().session.doc_mut().unwrap();
    let mut c = (*st.doc.chat).clone();
    c.messages.push(ChatMessage {
        id: id.into(),
        author: ChatAuthor { id: "u-bob".into(), name: "Bob".into(), color: "#D8553B".into() },
        at: deckcraft_engine::chat::now_ms() - 120_000,
        text: text.into(),
        ..Default::default()
    });
    st.doc = Arc::new(deckcraft_engine::model::Presentation { chat: Arc::new(c), ..(*st.doc).clone() });
    st.revision += 1;
}

fn chat_texts(h: &Harness<'static, SlideApp>) -> Vec<String> {
    h.state().session.active().unwrap().doc.chat.messages.iter().map(|m| m.text.clone()).collect()
}

#[test]
fn chat_button_unread_count_pane_and_menu() {
    let mut s = Session::with_new();
    s.execute("collab.setUser", &json!({"name": "Ann Lee"})).unwrap();
    let mut h = harness(s);
    assert!(h.query_by_label("Chat messages").is_none(), "no chat, no session: no chat button");
    // A note left in the file: the button shows, nothing unread (it's mine).
    h.state_mut().run("chat.send", json!({"text": "First note"})).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label("Chat messages").is_some());
    // Two from Bob, one mentioning me: unread count and the flash.
    from_bob(&mut h, "bob00001", "Looks good");
    from_bob(&mut h, "bob00002", "@Ann can you check slide 2? https://example.org");
    h.run_steps(2);
    assert!(h.query_by_label("Chat messages (2 unread)").is_some(), "the unread count");
    assert!(crate::chat_ui::flashing(h.state()), "a mention flashes the button");
    // Opening the pane reads them.
    h.get_by_label("Chat messages (2 unread)").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.pane.as_deref(), Some("chat"));
    assert!(h.query_by_label("Chat messages").is_some(), "nothing unread once open");
    assert!(h.query_by_label("Looks good").is_some());
    assert!(h.query_by_label("https://example.org").is_some(), "links are links");
    // The box has the focus: Enter sends, Shift+Enter doesn't.
    h.state_mut().chat.draft = "Checking now".into();
    h.run_steps(1);
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(chat_texts(&h).len(), 3, "Shift+Enter is a new line, not a send");
    h.state_mut().chat.draft = "Checking now".into();
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(chat_texts(&h).last().map(String::as_str), Some("Checking now"));
    assert!(h.state().chat.draft.is_empty());
    // Delete my own: "Message deleted" stays.
    let mine = h.state().session.active().unwrap().doc.chat.messages.last().unwrap().id.clone();
    h.state_mut().run("chat.delete", json!({"id": mine})).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label("Message deleted").is_some());
    // The menu: the Office note, and Clear chat… asks first.
    h.get_by_label("⋯").click();
    h.run_steps(2);
    assert!(h.query_by_label("Kept in the file; Office drops it when it saves").is_some());
    h.get_by_label("Clear chat…").click();
    h.run_steps(3);
    assert!(h.query_by_label("Remove the whole chat from the document for everyone?").is_some());
    assert_eq!(chat_texts(&h).len(), 4, "nothing removed before the answer");
    h.get_by_label("Clear chat").click();
    h.run_steps(3);
    assert!(chat_texts(&h).is_empty());
    assert!(h.state().session.active().unwrap().is_dirty(), "the chat change makes the presentation modified");
    // view.chat hides it again.
    h.state_mut().run("view.chat", json!({})).unwrap();
    h.run_steps(2);
    assert!(h.state().ui.pane.is_none());
}

#[test]
fn chat_in_a_session_unread_beside_the_people_and_typing() {
    let mut h = harness(Session::with_new());
    h.state_mut()
        .session
        .collab_join_in_memory(json!({"room": "ui:chat", "mode": "share", "user": {"id": "u-ann", "name": "Ann", "color": "#3B7DD8"}}))
        .unwrap();
    h.run_steps(2);
    assert!(h.query_by_label("Chat messages").is_some(), "in a session the chat button is there");
    // Bob joins (another session, connected in memory) and chats.
    let mut bob = Session::new();
    bob.collab_join_in_memory(json!({"room": "ui:chat", "mode": "join", "user": {"id": "u-bob", "name": "Bob", "color": "#D8553B"}})).unwrap();
    let relay = |h: &mut Harness<'static, SlideApp>, bob: &mut Session| {
        for _ in 0..20 {
            let to_bob = h.state_mut().session.collab_take_outgoing();
            let to_ann = bob.collab_take_outgoing();
            if to_bob.is_empty() && to_ann.is_empty() {
                break;
            }
            // The window's clock (its pump polls with it), so nobody's presence looks stale.
            let now = h.state().session.collab.now().max(bob.collab.now()) + 50;
            let push = |v: Vec<serde_json::Value>| -> Vec<Pushed> {
                v.into_iter()
                    .filter(|m| m["method"].as_str().is_some_and(|x| x != "collab/joined"))
                    .map(|m| Pushed { plugin: IN_MEMORY.into(), incoming: Incoming::Message(m) })
                    .collect()
            };
            bob.collab_poll_pushed(now, push(to_bob));
            h.state_mut().session.collab_poll_pushed(now, push(to_ann));
            h.run_steps(1);
        }
    };
    relay(&mut h, &mut bob);
    assert!(deckcraft_engine::collab::read_only(&bob).is_none(), "Bob joined");
    bob.execute("chat.send", &json!({"text": "Hello Ann"})).unwrap();
    bob.execute("chat.typing", &json!({"typing": true})).unwrap();
    relay(&mut h, &mut bob);
    h.run_steps(2);
    assert_eq!(chat_texts(&h), ["Hello Ann"]);
    assert!(h.query_by_label("Chat messages (1 unread)").is_some());
    assert!(h.query_by_label("Bob").is_some(), "the people circles");
    h.get_by_label("Chat messages (1 unread)").click();
    h.run_steps(3);
    assert!(h.query_by_label("Bob is typing…").is_some());
    assert!(h.query_by_label("Hello Ann").is_some());
}
