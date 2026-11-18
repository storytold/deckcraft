//! Two replicas against a room that was re-created at a **lower** version than the one a
//! participant already holds.
//!
//! A room can come back behind its clients: the cabinet re-creates a session whose store is gone
//! or unreadable, a room is restored from an older base, and a process that lost its journal
//! starts again from the file it was opened with. The tail of such a room cannot catch a replica
//! up — it starts above a head the replica never saw — so the room's own document is the
//! authority and has to replace the replica. These tests are the headless version of "insert a
//! shape after the room was re-created behind the client": the structural step must land, both
//! replicas must hold it, and the client must not spin through a resync per attempt.

use deckcraft_collab::{Claims, CollabDoc, DocSession, Hub, SessionConfig};
use deckcraft_collab_proto::{ClientInfo, ClientMsg, Role, ServerMsg};
use deckcraft_engine::Session;
use deckcraft_model::Presentation;
use serde_json::json;

struct Peer {
    doc: CollabDoc,
    engine: Session,
    actor: String,
}

/// A non-empty deck: a rectangle and an ellipse on one slide.
fn deck() -> Session {
    let mut s = Session::with_new();
    s.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).expect("rect");
    s.apply_remote("shape.insert", &json!({"preset": "ellipse", "rect": [400, 100, 120, 100]})).expect("ellipse");
    s
}

fn shapes(p: &Presentation) -> usize {
    p.slides.iter().map(|slide| slide.shapes.len()).sum()
}

fn room(document: &Presentation) -> Hub {
    Hub::new(DocSession::from_document("d", document.clone(), SessionConfig::default()).expect("session"))
}

fn join(room: &mut Hub, actor: &str, role: Role) -> Peer {
    let mut doc = CollabDoc::new("d");
    let mut engine = deck();
    let hello = doc.connect(&engine, "jwt", ClientInfo { name: "t".into(), ver: "1".into() });
    let ClientMsg::Hello(hello) = hello else { panic!("the client sends the handshake first") };
    let welcome = room.join(actor, &hello, Claims { actor_id: actor.into(), role }).expect("join");
    doc.on_msg(&mut engine, ServerMsg::Welcome(welcome));
    Peer { doc, engine, actor: actor.into() }
}

/// One reconnect of an existing replica: a fresh handshake against `room`.
fn rejoin(room: &mut Hub, peer: &mut Peer) {
    peer.doc.disconnected();
    let hello = peer.doc.connect(&peer.engine, "jwt", ClientInfo { name: "t".into(), ver: "1".into() });
    let ClientMsg::Hello(hello) = hello else { panic!("the client sends the handshake first") };
    let welcome = room.join(&peer.actor, &hello, Claims { actor_id: peer.actor.clone(), role: Role::Editor }).expect("join");
    peer.doc.on_msg(&mut peer.engine, ServerMsg::Welcome(welcome));
}

/// Move everything the client queued to the room and every answer back. Returns how many
/// `resync` requests the client made.
fn pump(room: &mut Hub, peer: &mut Peer, others: &mut [&mut Peer]) -> usize {
    let mut resyncs = 0;
    for _ in 0..40 {
        let mut worked = false;
        while let Some(msg) = peer.doc.next_outgoing() {
            worked = true;
            resyncs += usize::from(matches!(msg, ClientMsg::Resync { .. }));
            room.receive(&peer.actor, msg).expect("the room takes the message");
        }
        let mut answers = Vec::new();
        while let Some(answer) = room.poll(&peer.actor) {
            answers.push(answer);
        }
        for answer in answers {
            worked = true;
            peer.doc.on_msg(&mut peer.engine, answer.clone());
            for other in others.iter_mut() {
                other.doc.on_msg(&mut other.engine, answer.clone());
            }
        }
        if !worked {
            break;
        }
    }
    resyncs
}

fn submit_insert(peer: &mut Peer, id: &str) -> usize {
    peer.doc.submit(&mut peer.engine, "shape.insert", &json!({"preset": "rect", "rect": [10, 10, 50, 50]}), id).expect("queued");
    shapes(&peer.engine.doc().expect("doc").doc)
}

#[test]
fn an_insert_lands_when_the_room_came_back_behind_the_client() {
    let document = deck().doc().expect("doc").doc.as_ref().clone();
    let mut first = room(&document);
    let mut a = join(&mut first, "alice", Role::Editor);
    let before = submit_insert(&mut a, "a:1");
    assert_eq!(pump(&mut first, &mut a, &mut []), 0, "a replica at the head needs no resync");
    assert_eq!(first.version(), 1);
    assert_eq!(a.doc.version(), 1);
    assert_eq!(shapes(first.session().document().expect("doc")), before + 1, "the room holds the inserted shape");

    // The room is gone and the cabinet opens the file again: a new room, at version 0, holding the
    // document the file has. The client still holds version 1.
    let mut second = room(&document);
    assert_eq!(second.version(), 0);
    rejoin(&mut second, &mut a);
    let resyncs = pump(&mut second, &mut a, &mut []);
    assert_eq!(a.doc.version(), 0, "the replica is rebuilt from the room that is behind it");
    assert_eq!(resyncs, 1, "one resync repairs the divergence, not one per attempt");

    // A second participant joins the re-created room at its head.
    let mut b = join(&mut second, "bob", Role::Editor);
    assert_eq!(pump(&mut second, &mut b, &mut [&mut a]), 0);

    // The structural step of the client that was ahead lands on the room and on both replicas.
    // The room holds the document the file was opened with, so the count is that plus this one
    // insert — the shape the first room had went away with it.
    let initial = shapes(&document);
    submit_insert(&mut a, "a:2");
    let resyncs = pump(&mut second, &mut a, &mut [&mut b]);
    assert_eq!(resyncs, 0, "no resync is owed once the replica is the room's");
    assert_eq!(second.version(), 1);
    assert_eq!(a.doc.version(), 1);
    assert_eq!(b.doc.version(), 1);
    assert_eq!(shapes(second.session().document().expect("doc")), initial + 1, "the room applied the insert");
    assert_eq!(shapes(&a.engine.doc().expect("doc").doc), initial + 1, "the author sees it");
    assert_eq!(shapes(&b.engine.doc().expect("doc").doc), initial + 1, "and so does the other replica");
}

#[test]
fn a_structural_step_over_a_stale_base_is_still_resolved_by_the_rfc() {
    // The rule for a later structural step is unchanged: it needs the head, a stale one is
    // refused as retryable and comes back through a resync (RFC §4.3).
    let document = deck().doc().expect("doc").doc.as_ref().clone();
    let mut room_now = room(&document);
    let mut a = join(&mut room_now, "alice", Role::Editor);
    let mut b = join(&mut room_now, "bob", Role::Editor);
    pump(&mut room_now, &mut a, &mut [&mut b]);
    pump(&mut room_now, &mut b, &mut [&mut a]);

    // Alice's insert reaches the room; Bob's link has not delivered the answer to his replica yet,
    // so his copy is still at version 0.
    submit_insert(&mut a, "a:1");
    pump(&mut room_now, &mut a, &mut []);
    assert_eq!(room_now.version(), 1);
    assert_eq!(b.doc.version(), 0, "Bob has not seen the answer");

    // Bob's structural step leaves with the stale base 0: the room refuses it as retryable, the
    // client resyncs, replays the tail and sends the operation again at the head it now holds.
    let before_b = shapes(&b.engine.doc().expect("doc").doc);
    submit_insert(&mut b, "b:1");
    let resyncs = pump(&mut room_now, &mut b, &mut [&mut a]);
    assert_eq!(resyncs, 1, "one resync repairs the stale base");
    assert_eq!(room_now.version(), 2);
    assert_eq!(b.doc.version(), 2);
    assert_eq!(shapes(&b.engine.doc().expect("doc").doc), before_b + 2, "Bob's replica replayed the tail and applied its own");
    assert_eq!(shapes(&a.engine.doc().expect("doc").doc), before_b + 2, "and so did Alice's");
}
