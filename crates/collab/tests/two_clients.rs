//! Two clients against one authoritative session, in one process and without a socket.
//!
//! This drives the three pieces together — `ClientSession` → `DocSession` → `ClientSession` — the
//! way the transport will: handshake, operations, refusal with a resync, resend, dedup. It is the
//! headless version of "two participants edit one deck" from `TASK.md` §8.

use deckcraft_collab::{ClientSession, DocSession, SessionConfig, SubmitError};
use deckcraft_collab_proto::{
    Capabilities, ClientInfo, ClientMsg, Limits, OpEnvelope, OpKind, Payload, Role, ServerMsg, ShapeKey, SlideKey, SnapshotMode, SnapshotRef, Target,
    Welcome,
};
use deckcraft_engine::Session;
use deckcraft_model::Presentation;
use serde_json::json;

struct Peer {
    client: ClientSession,
    actor: String,
}

fn deck() -> Presentation {
    let mut s = Session::with_new();
    let title = s.doc().expect("doc").doc.slides[0].id;
    s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("blank");
    s.apply_remote("slide.delete", &json!({"ids": [title.0]})).expect("drop title");
    s.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).expect("rect");
    s.apply_remote("shape.insert", &json!({"preset": "ellipse", "rect": [400, 100, 120, 100]})).expect("ellipse");
    s.doc().expect("doc").doc.as_ref().clone()
}

fn keys(server: &DocSession) -> (SlideKey, ShapeKey, ShapeKey) {
    let doc = server.document().expect("doc");
    let slide = doc.slides.first().expect("slide");
    let k = server.keys();
    (
        k.slide_key(slide.id).expect("slide key").clone(),
        k.shape_key(slide.id, slide.shapes[0].id).expect("shape 0").clone(),
        k.shape_key(slide.id, slide.shapes[1].id).expect("shape 1").clone(),
    )
}

fn welcome(version: u64, actor: &str, role: Role) -> ServerMsg {
    ServerMsg::Welcome(Welcome {
        v: deckcraft_collab_proto::PROTO_VERSION,
        session_id: "s1".into(),
        actor_id: actor.into(),
        role,
        lease: None,
        version,
        snapshot: SnapshotRef { mode: SnapshotMode::BaseAndTail, cursor: format!("tail@{version}") },
        capabilities: Capabilities {
            kinds: vec![OpKind::Command, OpKind::Structural],
            limits: Limits::default(),
            commands: vec!["shape.move".into()],
        },
    })
}

/// Handshake, as the transport does: the client sends `hello`, the server answers `welcome`.
fn join(server: &DocSession, actor: &str, role: Role) -> Peer {
    let mut client = ClientSession::new(server.file_id());
    let hello = client.connect("jwt", ClientInfo { name: "deckcraft-test".into(), ver: "0.1".into() });
    assert!(matches!(hello, ClientMsg::Hello(_)));
    client.on_msg(welcome(server.version(), actor, role));
    Peer { client, actor: actor.into() }
}

/// Move every message the client has for the server, and the answers back. Stops when the client
/// has nothing left to send.
fn pump(server: &mut DocSession, peer: &mut Peer, others: &mut [&mut Peer]) {
    for _ in 0..10 {
        let Some(msg) = peer.client.next_outgoing() else { return };
        match msg {
            ClientMsg::Op(op) => match server.apply(&peer.actor, &op) {
                Ok(applied) => {
                    peer.client.on_msg(ServerMsg::Applied(applied.clone()));
                    for other in others.iter_mut() {
                        other.client.on_msg(ServerMsg::Applied(applied.clone()));
                    }
                }
                Err(rejected) => {
                    peer.client.on_msg(ServerMsg::Rejected(rejected));
                }
            },
            ClientMsg::Resync { last_version } => {
                let snapshot = server.resync(last_version);
                peer.client.on_msg(ServerMsg::Snapshot(snapshot));
            }
            ClientMsg::Ping | ClientMsg::Commit { .. } | ClientMsg::Presence(_) | ClientMsg::Hello(_) | ClientMsg::Undo | ClientMsg::Redo => return,
        }
    }
}

#[test]
fn a_picture_is_stored_once_and_reaches_both_clients() {
    let mut server = DocSession::from_document("d", deck(), SessionConfig::default()).expect("session");
    let bytes = png();
    let sha = server.put_media("red.png", bytes.clone()).expect("media");
    assert_eq!(server.put_media("red.png", bytes).expect("same bytes"), sha, "content-addressed: one object");
    assert_eq!(server.media_entries().len(), 1);

    let (slide, _first, _second) = keys(&server);
    let mut a = join(&server, "alice", Role::Editor);
    let mut b = join(&server, "bob", Role::Editor);
    let op = OpEnvelope {
        op_id: "pic-1".into(),
        doc: "d".into(),
        base_version: 0,
        kind: OpKind::Structural,
        target: Target { slide: Some(slide), ..Target::default() },
        payload: Payload::PictureInsert { sha256: sha.clone(), name: "red.png".into(), rect: Some([60.0, 60.0, 80.0, 80.0]) },
        history: None,
    };
    a.client.submit(op).expect("submit picture");
    pump(&mut server, &mut a, &mut [&mut b]);

    assert_eq!(server.version(), 1);
    assert_eq!(a.client.version(), 1);
    assert_eq!(b.client.version(), 1);
    let doc = server.document().expect("doc");
    assert!(doc.slides[0].shapes.iter().any(|s| matches!(s.kind, deckcraft_model::ShapeKind::Picture { .. })), "the picture is on the slide");
    assert_eq!(server.media_entries().len(), 1);
}

fn png() -> Vec<u8> {
    vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0, 8, 8, 2, 0, 0, 0, 75, 109, 41, 220, 0, 0, 0, 17, 73, 68,
        65, 84, 120, 156, 99, 184, 163, 161, 129, 21, 49, 12, 45, 9, 0, 123, 123, 75, 1, 251, 162, 81, 240, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96,
        130,
    ]
}

fn move_op(slide: &SlideKey, shape: &ShapeKey, x: f64) -> OpEnvelope {
    OpEnvelope {
        op_id: format!("op-{x}"),
        doc: "d".into(),
        base_version: 0,
        kind: OpKind::Command,
        target: Target { slide: Some(slide.clone()), shape: Some(shape.clone()), ..Target::default() },
        payload: Payload::ShapeMove { x, y: 100.0 },
        history: None,
    }
}

#[test]
fn two_editors_converge_and_both_settle() {
    let mut server = DocSession::from_document("d", deck(), SessionConfig::default()).expect("session");
    let (slide, first, second) = keys(&server);
    let mut a = join(&server, "alice", Role::Editor);
    let mut b = join(&server, "bob", Role::Editor);

    a.client.submit(move_op(&slide, &first, 300.0)).expect("submit a");
    pump(&mut server, &mut a, &mut [&mut b]);
    b.client.submit(move_op(&slide, &second, 600.0)).expect("submit b");
    pump(&mut server, &mut b, &mut [&mut a]);

    assert_eq!(server.version(), 2);
    assert_eq!(a.client.version(), 2);
    assert_eq!(b.client.version(), 2);
    assert_eq!(a.client.in_flight(), 0);
    assert_eq!(b.client.in_flight(), 0);
    let doc = server.document().expect("doc");
    assert_eq!(doc.slides[0].shapes[0].xfrm.expect("xfrm").x, 300.0);
    assert_eq!(doc.slides[0].shapes[1].xfrm.expect("xfrm").x, 600.0);
}

#[test]
fn a_structural_operation_over_a_stale_base_recovers_through_resync() {
    let mut server = DocSession::from_document("d", deck(), SessionConfig::default()).expect("session");
    let (slide, _first, second) = keys(&server);
    let mut a = join(&server, "alice", Role::Editor);
    let mut b = join(&server, "bob", Role::Editor);

    // Bob moves a shape: the server is at version 1.
    b.client.submit(move_op(&slide, &second, 500.0)).expect("submit b");
    pump(&mut server, &mut b, &mut [&mut a]);
    assert_eq!(server.version(), 1);

    // Alice still believes she is at 0 and asks to delete that shape structurally.
    let delete = OpEnvelope {
        op_id: "alice-delete".into(),
        doc: "d".into(),
        base_version: 0,
        kind: OpKind::Structural,
        target: Target { slide: Some(slide), shape: Some(second), ..Target::default() },
        payload: Payload::ShapeDelete {},
        history: None,
    };
    a.client.submit(delete).expect("submit delete");
    pump(&mut server, &mut a, &mut [&mut b]);

    // The refusal was retryable: the client asked for a resync, rebased on the head and the
    // delete went through. Nothing was lost and nothing applied twice.
    assert_eq!(server.version(), 2);
    assert_eq!(a.client.version(), 2);
    assert_eq!(a.client.in_flight(), 0);
    assert_eq!(a.client.queued(), 0);
}

#[test]
fn a_resend_is_settled_but_changes_nothing() {
    let mut server = DocSession::from_document("d", deck(), SessionConfig::default()).expect("session");
    let (slide, first, _second) = keys(&server);
    let mut a = join(&server, "alice", Role::Editor);
    let op = move_op(&slide, &first, 350.0);
    a.client.submit(op.clone()).expect("submit");
    pump(&mut server, &mut a, &mut []);
    let after_first = server.document().expect("doc").clone();

    let again = server.apply("alice", &op).expect("dedup");
    assert_eq!(again.version, 1, "the stored answer comes back");
    assert_eq!(server.version(), 1, "and nothing applied again");
    assert_eq!(*server.document().expect("doc"), after_first);
}

#[test]
fn media_and_format_slices_go_through_the_server() {
    let mut server = DocSession::from_document("d", deck(), SessionConfig::default()).expect("session");
    let (slide, first, second) = keys(&server);
    let mut a = join(&server, "alice", Role::Editor);
    let mut b = join(&server, "bob", Role::Editor);

    let steps = vec![
        (Payload::ShapeFill { fill: deckcraft_collab_proto::FillSpec::Solid { color: "#ff0000".into() } }, OpKind::Command, first.clone(), 0u64),
        (
            Payload::ShapeLine { line: deckcraft_collab_proto::LineSpec { color: Some("#0000ff".into()), width: Some(2.0), ..Default::default() } },
            OpKind::Command,
            second.clone(),
            1,
        ),
        (Payload::FormatSet { format: deckcraft_collab_proto::FormatCmd::Bold, args: json!({"on": true}) }, OpKind::Command, first.clone(), 2),
        (Payload::SlideDuplicate {}, OpKind::Structural, first.clone(), 3),
    ];
    for (n, (payload, kind, shape, base)) in steps.into_iter().enumerate() {
        let op = OpEnvelope {
            op_id: format!("op-{n}"),
            doc: "d".into(),
            base_version: base,
            kind,
            target: Target { slide: Some(slide.clone()), shape: Some(shape), ..Target::default() },
            payload,
            history: None,
        };
        a.client.submit(op).expect("submit");
        pump(&mut server, &mut a, &mut [&mut b]);
    }

    assert_eq!(server.version(), 4, "every slice is one authoritative step");
    assert_eq!(b.client.version(), 4);
    let doc = server.document().expect("doc");
    assert_eq!(doc.slides.len(), 2, "the copy landed");
    let first_shape = &doc.slides[0].shapes[0];
    assert!(matches!(first_shape.fill, Some(deckcraft_model::Fill::Solid { .. })), "the fill came from the confirmed command");
    assert_eq!(doc.slides[0].shapes[1].line.as_ref().and_then(|l| l.width), Some(2.0), "the outline is a final value");
}

#[test]
fn a_viewer_cannot_write() {
    let server = DocSession::from_document("d", deck(), SessionConfig::default()).expect("session");
    let (_slide, first, _) = keys(&server);
    let mut viewer = join(&server, "victor", Role::Viewer);
    let slide = _slide;
    assert_eq!(viewer.client.submit(move_op(&slide, &first, 300.0)), Err(SubmitError::ReadOnly));
}

#[test]
fn undo_of_one_editor_leaves_the_other_alone() {
    let mut server = DocSession::from_document("d", deck(), SessionConfig::default()).expect("session");
    let (slide, first, second) = keys(&server);
    let mut a = join(&server, "alice", Role::Editor);
    let mut b = join(&server, "bob", Role::Editor);

    a.client.submit(move_op(&slide, &first, 300.0)).expect("submit a");
    pump(&mut server, &mut a, &mut [&mut b]);
    b.client.submit(move_op(&slide, &second, 650.0)).expect("submit b");
    pump(&mut server, &mut b, &mut [&mut a]);

    let undo = server.undo("alice", "alice-undo").expect("undo");
    a.client.submit(undo).expect("submit undo");
    pump(&mut server, &mut a, &mut [&mut b]);

    let doc = server.document().expect("doc");
    assert_eq!(doc.slides[0].shapes[0].xfrm.expect("xfrm").x, 100.0, "alice's move is taken back");
    assert_eq!(doc.slides[0].shapes[1].xfrm.expect("xfrm").x, 650.0, "bob's move stays");
    assert_eq!(server.version(), 3);
}
