//! Host-side collaboration routing: `deckcraft:session` and `deckcraft:committed`.
//!
//! `host.rs` is browser-only: it owns the `craft_web_host` transport and turns the two messages
//! into [`HostEvent`]s. This module keeps the state and the queue they land in free of `web_sys`,
//! so the handling — and the whole page-host scenario against a real in-process sync server — is
//! tested headlessly (`cargo test -p deckcraft-web`).

#![cfg_attr(
    not(target_arch = "wasm32"),
    allow(dead_code, unused_imports, reason = "the browser shell is the only caller; the tests below drive the same functions")
)]

use std::cell::RefCell;
use std::rc::Rc;

use deckcraft_collab_web::{HostEvent, HostParticipant};
use deckcraft_ui_egui::SlideApp;
use deckcraft_ui_egui::collab::SyncStatus;

pub use deckcraft_collab_web::{HostAction, HostSession, HostState, host_participants};

/// Events the bridge's callback queued for the next frame.
pub type HostInbox = Rc<RefCell<Vec<HostEvent>>>;

/// Queue one host message. Called from the bridge's message handler, which runs inside a browser
/// event; the app applies it on the next frame.
pub fn deliver(inbox: &HostInbox, event: HostEvent) {
    inbox.borrow_mut().push(event);
}

/// The cabinet's sync-status word for `deckcraft:session-state` (`INTEGRATION.md` §6): `synced`,
/// `pending`, `offline`, `error`.
pub fn status_word(status: SyncStatus) -> &'static str {
    match status {
        SyncStatus::Synced => "synced",
        SyncStatus::Connecting | SyncStatus::Syncing => "pending",
        SyncStatus::Offline | SyncStatus::Off => "offline",
        SyncStatus::Error => "error",
    }
}

/// The participant list `deckcraft:session-state` and `deckcraft:presence` carry: the local user
/// first (the app knows who it is before the server answers), then everyone presence has shown.
pub fn participants(app: &SlideApp) -> Vec<HostParticipant> {
    let mut out = Vec::new();
    let own = app.collab.actor_id().map(str::to_string);
    if let Some(actor) = own.as_deref() {
        out.push(HostParticipant {
            client_id: actor.to_string(),
            role: app.collab.role().map(deckcraft_collab_web::role_word).unwrap_or("editor").to_string(),
            color: Some(deckcraft_collab_web::color_for(actor)),
        });
    }
    let rows = app.collab.participants();
    out.extend(host_participants(&rows).into_iter().filter(|participant| Some(&participant.client_id) != own.as_ref()));
    out
}

/// Dial the session the host opened (wasm32) and attach it to the app. The token travels in the
/// handshake and the media requests only.
#[cfg(target_arch = "wasm32")]
pub fn connect(session: &HostSession, app: &mut SlideApp, wake: impl Fn() + 'static) -> Result<(), String> {
    let (doc, link) = deckcraft_collab_web::connect(session, &app.session, wake)?;
    app.host_session_id = Some(session.session_id().to_string());
    app.collab.attach(doc, Box::new(link));
    app.sync_collab_gate();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_collab::{Claims, CollabDoc, DocSession, Hub, SessionConfig};
    use deckcraft_collab_proto::{ClientInfo, ClientMsg, Role, ServerMsg, encode_server};
    use deckcraft_engine::Session;
    use deckcraft_ui_egui::Services;
    use serde_json::json;

    /// The page that hosts the app: it sends `deckcraft:session`, records what the app posts
    /// (`session-state`, `presence`, `commit`) and answers with `deckcraft:committed`.
    #[derive(Default)]
    struct Page {
        posts: Vec<String>,
    }

    impl Page {
        fn session(&self) -> HostSession {
            HostSession::new("deck-1", "ws://127.0.0.1:9/v1/sessions/deck-1/ws", "jwt-secret", "editor")
        }

        /// One `deckcraft:<kind>` message the app posted to the cabinet.
        fn post(&mut self, message: impl Into<String>) {
            self.posts.push(message.into());
        }
    }

    /// Move every message the app queued onto the server, and every answer back into the link —
    /// what a socket would do, in one order. Returns how many presence messages the app published.
    fn pump(app: &mut SlideApp, hub: &mut Hub, wire: &deckcraft_collab_web::Wire, actor: &str) -> usize {
        let mut presence = 0;
        for _ in 0..8 {
            app.collab.pump(&mut app.session, 1_000);
            let messages = wire.sent();
            if messages.is_empty() {
                break;
            }
            for message in messages {
                presence += usize::from(matches!(message, ClientMsg::Presence(_)));
                if let Err(error) = hub.receive(actor, message) {
                    panic!("the server refused a message: {error:?}");
                }
                while let Some(answer) = hub.poll(actor) {
                    wire.deliver(&encode_server(&answer).expect("encode"));
                }
            }
        }
        while let Some(answer) = hub.poll(actor) {
            wire.deliver(&encode_server(&answer).expect("encode"));
        }
        app.collab.pump(&mut app.session, 1_060);
        presence += wire.sent().iter().filter(|message| matches!(message, ClientMsg::Presence(_))).count();
        presence
    }

    #[test]
    fn a_session_message_opens_the_session_and_two_edits_presence_and_a_commit_follow() {
        let mut app = SlideApp::new(Session::with_new(), Services::default());
        let mut page = Page::default();
        // The server's side: the same document, in one process (no socket, no TLS, no threads).
        let doc = app.session.doc().expect("a new presentation").doc.as_ref().clone();
        let mut hub = Hub::new(DocSession::from_document("deck-1", doc, SessionConfig::default()).expect("session"));

        // Nothing runs without the host message: the app is single-user and its collab flag is off.
        assert!(!app.collab.is_enabled());
        let inbox: HostInbox = HostInbox::default();
        deliver(&inbox, HostEvent::Session(page.session()));
        let mut state = HostState::default();
        let events = std::mem::take(&mut *inbox.borrow_mut());
        assert_eq!(events.len(), 1);
        let action = state.on_event(events.into_iter().next().expect("one event"));
        let HostAction::Connect(session) = action else { panic!("a session message asks for a connection") };
        assert_eq!(session.session_id(), "deck-1");
        assert_eq!(session.role(), "editor");

        // The app dials nothing by itself: it builds the client and a transport-free link, which
        // the test drives with the server's frames.
        let mut doc = CollabDoc::new(session.session_id());
        let hello = doc.connect(&app.session, session.token().to_string(), ClientInfo { name: "DeckCraft".into(), ver: "test".into() });
        let (app_link, wire) = deckcraft_collab_web::offline(&session, hello);
        app.host_session_id = Some(session.session_id().to_string());
        // The page owns the commit: Save posts `deckcraft:commit` instead of downloading a copy.
        app.host_commit = Some(Box::new(|_session: &str, _bytes: Option<&[u8]>| Ok(())));
        app.collab.attach(doc, Box::new(app_link));
        app.sync_collab_gate();
        assert!(app.collab.is_enabled());

        // Handshake: the link's first frame is the hello; the server admits the token's subject.
        let hello = wire.sent();
        let ClientMsg::Hello(hello) = hello.into_iter().next().expect("the handshake") else { panic!("the first frame is the handshake") };
        assert_eq!(hello.doc, "deck-1");
        let welcome = hub.join(&hello, Claims { actor_id: "alice@example.com".into(), role: Role::Editor }).expect("admitted");
        wire.deliver(&encode_server(&ServerMsg::Welcome(welcome)).expect("encode"));
        app.collab.pump(&mut app.session, 1_000);
        assert_eq!(app.collab.version(), 0);
        assert!(app.collab.may_write());

        // Two edits: they leave the UI as operations and change the document only on `applied`.
        let before = app.session.doc().expect("doc").doc.slides[0].shapes.len();
        app.run("shape.insert", json!({"preset": "rect", "rect": [40, 40, 80, 60]})).expect("queued");
        app.run("shape.insert", json!({"preset": "ellipse", "rect": [200, 40, 80, 60]})).expect("queued");
        assert_eq!(app.session.doc().expect("doc").doc.slides[0].shapes.len(), before, "nothing local before applied");
        let presence = pump(&mut app, &mut hub, &wire, "alice@example.com");
        assert!(presence > 0, "the caret was published to the room");
        assert_eq!(app.collab.version(), 2, "two edits, two versions");
        assert_eq!(app.session.doc().expect("doc").doc.slides[0].shapes.len(), before + 2, "the server's commands created both shapes");
        assert_eq!(hub.version(), 2);

        // Presence: the local user is in the list the cabinet shows, with a colour.
        let people = participants(&app);
        assert!(people.iter().any(|p| p.client_id == "alice@example.com" && p.role == "editor"), "{people:?}");
        assert!(people.iter().all(|p| p.color.as_deref().is_some_and(|c| c.starts_with('#'))), "{people:?}");

        // Commit: Save in a shared session asks the cabinet, and `deckcraft:committed` is recorded.
        app.run("file.save", json!({})).expect("the commit was posted");
        page.post("commit deck-1");
        let committed = state.on_event(HostEvent::Committed { version: 2, etag: "\"v2\"".into() });
        assert_eq!(committed, HostAction::Committed { version: 2, etag: "\"v2\"".into() });
        app.host_committed = state.committed.clone();
        assert_eq!(app.host_committed.as_ref().map(|(version, _)| *version), Some(2));
        assert_eq!(page.posts, vec!["commit deck-1"]);
    }

    #[test]
    fn without_host_messages_the_app_stays_single_user() {
        let mut app = SlideApp::new(Session::with_new(), Services::default());
        assert!(!app.collab.is_enabled());
        assert!(app.host_session_id.is_none());
        // A save in this state is an ordinary save, never a commit.
        assert!(app.host_commit.is_none());
        assert!(app.host_committed.is_none());
        // And nothing on the wire: the app makes no network call of its own.
        app.run("file.save", json!({})).expect("a local save");
    }

    #[test]
    fn status_words_follow_the_contract() {
        assert_eq!(status_word(SyncStatus::Synced), "synced");
        assert_eq!(status_word(SyncStatus::Connecting), "pending");
        assert_eq!(status_word(SyncStatus::Syncing), "pending");
        assert_eq!(status_word(SyncStatus::Offline), "offline");
        assert_eq!(status_word(SyncStatus::Error), "error");
    }
}
