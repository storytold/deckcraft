//! #414: a browser file pick finishes on the document, slide and operation that started it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};

use deckcraft_model::ShapeKind;
use serde_json::json;

use super::{Inbox, InboxEvent, InboxFile, Services, SlideApp};
use crate::Session;

fn png() -> Vec<u8> {
    // 1×1 RGB PNG. Enough for `insert.picture` to accept it.
    const HEX: &str =
        "89504e470d0a1a0a0000000d4948445200000001000000010802000000907753de0000000c49444154789c63f8cfc0000003010100c9fe92ef0000000049454e44ae426082";
    (0..HEX.len()).step_by(2).map(|i| u8::from_str_radix(&HEX[i..i + 2], 16).expect("hex")).collect()
}

fn wav() -> Vec<u8> {
    let data = [0u8; 8];
    let mut b = b"RIFF".to_vec();
    b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&8000u32.to_le_bytes());
    b.extend_from_slice(&16000u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(data.len() as u32).to_le_bytes());
    b.extend_from_slice(&data);
    b
}

fn setup() -> (SlideApp, Inbox, Arc<Mutex<Vec<(String, u64)>>>) {
    let inbox: Inbox = Inbox::default();
    let started = Arc::new(Mutex::new(Vec::new()));
    let record = started.clone();
    let services = Services {
        inbox: Some(inbox.clone()),
        open_async: Some(Box::new(move |purpose, id| {
            record.lock().expect("request log").push((purpose.to_string(), id));
        })),
        ..Default::default()
    };
    (SlideApp::new(Session::with_new(), services), inbox, started)
}

fn deliver(inbox: &Inbox, id: u64, name: &str, bytes: Vec<u8>) {
    inbox.lock().expect("inbox").push(InboxEvent::File(InboxFile { id, name: name.into(), bytes }));
}

fn pictures(app: &SlideApp, doc: u64, slide: deckcraft_model::SlideId) -> usize {
    let st = app.session.documents().iter().find(|d| d.uid == doc).expect("document");
    let i = st.doc.slide_index(slide).expect("slide");
    st.doc.slides[i].shapes.iter().filter(|s| matches!(s.kind, ShapeKind::Picture { .. })).count()
}

fn audios(app: &SlideApp, doc: u64, slide: deckcraft_model::SlideId) -> usize {
    let st = app.session.documents().iter().find(|d| d.uid == doc).expect("document");
    let i = st.doc.slide_index(slide).expect("slide");
    st.doc.slides[i].shapes.iter().filter(|s| matches!(s.kind, ShapeKind::Media(ref m) if !m.video)).count()
}

/// Two picks finish in reverse order, after the user has changed slide and document.
/// Each file uses the operation that was requested and lands on the slide that asked.
#[test]
fn late_picker_files_keep_their_document_slide_and_operation() {
    let (mut app, inbox, started) = setup();
    let uid_a = app.session.active().unwrap().uid;
    let slide0 = app.session.active().unwrap().current_slide().unwrap().id;
    app.session.execute("slide.new", &json!({})).unwrap();
    let slide1 = app.session.active().unwrap().current_slide().unwrap().id;
    assert_ne!(slide0, slide1);

    // Audio was asked for on slide 1; the picture and the outline on slide 0.
    app.pick_and_insert("audio", "insert.audio");
    app.session.execute("slide.go", &json!({"id": slide0.0})).unwrap();
    app.pick_and_insert("picture", "insert.picture");
    app.pick_and_open();

    app.session.execute("file.new", &json!({})).unwrap();
    let uid_b = app.session.active().unwrap().uid;
    assert_ne!(uid_a, uid_b);
    let shapes_b = app.session.active().unwrap().doc.slides[0].shapes.len();
    // A video was requested on B. Delivering a PNG must not insert a picture anywhere.
    app.pick_and_insert("video", "insert.video");

    let log = started.lock().expect("request log").clone();
    assert_eq!(log.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(), vec!["audio", "picture", "open", "video"]);
    let audio_id = log[0].1;
    let picture_id = log[1].1;
    let outline_id = log[2].1;
    let video_id = log[3].1;

    // The video request completes first, while B is still current. A PNG must not become a picture.
    deliver(&inbox, video_id, "clip.mp4", png());
    app.drain_inbox();
    let status = app.status.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
    assert!(status.contains("not a supported video") || status.contains("no video"), "{status}");
    assert_eq!(app.session.active().unwrap().doc.slides[0].shapes.len(), shapes_b);

    // The earlier picks then finish in reverse order.
    deliver(&inbox, outline_id, "talk.md", b"Brought Later\n".to_vec());
    deliver(&inbox, picture_id, "shot.png", png());
    deliver(&inbox, audio_id, "tone.wav", wav());
    app.drain_inbox();

    assert_eq!(app.session.active().unwrap().uid, uid_b, "the view stays on the document the user switched to");
    assert_eq!(app.session.active().unwrap().doc.slides[0].shapes.len(), shapes_b, "the video request must not insert the PNG here");
    assert_eq!(pictures(&app, uid_b, app.session.documents().iter().find(|d| d.uid == uid_b).unwrap().doc.slides[0].id), 0);

    let status = app.status.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
    assert_eq!(pictures(&app, uid_a, slide0), 1, "{status}");
    assert_eq!(pictures(&app, uid_a, slide1), 0);
    assert_eq!(audios(&app, uid_a, slide1), 1);
    assert_eq!(audios(&app, uid_a, slide0), 0);
    let a = app.session.documents().iter().find(|d| d.uid == uid_a).unwrap();
    assert!(a.doc.slides.iter().any(|s| s.title() == "Brought Later"), "the outline stays on the document that opened it");
    assert_eq!(a.doc.slides.len(), 3);
    // The user had moved to slide 0 of A before switching documents; B is what's on screen.
    assert_eq!(app.session.active().unwrap().selection.slide, 0);
}

/// Closing the presentation, or dismissing the picker, drops the file instead of inserting
/// it into whatever is open now.
#[test]
fn a_closed_or_cancelled_request_does_not_insert_into_the_active_document() {
    let (mut app, inbox, started) = setup();
    let slide = app.session.active().unwrap().current_slide().unwrap().id;
    app.pick_and_insert("picture", "insert.picture");
    app.session.execute("slide.new", &json!({})).unwrap();
    let kept = app.session.active().unwrap().current_slide().unwrap().id;
    app.session.execute("slide.delete", &json!({"ids": [slide.0]})).unwrap();
    app.pick_and_insert("picture", "insert.picture");
    let log = started.lock().expect("request log").clone();
    let deleted_slide = log[0].1;
    let dismiss = log[1].1;

    inbox.lock().expect("inbox").push(InboxEvent::Cancelled { id: dismiss });
    deliver(&inbox, dismiss, "late.png", png());
    deliver(&inbox, deleted_slide, "gone.png", png());
    // An id that was never started must not insert either.
    deliver(&inbox, 999, "stray.png", png());
    app.drain_inbox();

    assert_eq!(pictures(&app, app.session.active().unwrap().uid, kept), 0);
    let status = app.status.as_ref().map(|(t, _)| t.as_str()).unwrap_or("");
    assert!(status.contains("deleted"), "{status}");

    app.session.execute("file.close", &json!({})).unwrap();
    app.session.execute("file.new", &json!({})).unwrap();
    // The deleted-slide request was already consumed. Start another and close before it arrives.
    let closed = app.session.active().unwrap().uid;
    app.pick_and_insert("audio", "insert.audio");
    let audio_id = started.lock().expect("request log").last().unwrap().1;
    app.session.execute("file.close", &json!({})).unwrap();
    app.session.execute("file.new", &json!({})).unwrap();
    let other = app.session.active().unwrap().uid;
    assert_ne!(closed, other);
    deliver(&inbox, audio_id, "tone.wav", wav());
    app.drain_inbox();
    let st = app.session.documents().iter().find(|d| d.uid == other).unwrap();
    let n = st.doc.slides.iter().flat_map(|s| s.shapes.iter()).filter(|s| matches!(s.kind, ShapeKind::Media(_))).count();
    assert_eq!(n, 0, "a file for a closed presentation is not inserted into the one now open");
    let status = app.status.as_ref().map(|(t, _)| t.as_str()).unwrap_or("");
    assert!(status.contains("closed"), "{status}");
}

/// A drop is bound to the slide under the pointer at drop time, not the slide current when
/// the bytes have been read.
#[test]
fn a_dropped_file_stays_on_the_slide_it_was_dropped_on() {
    let (mut app, inbox, _) = setup();
    let uid = app.session.active().unwrap().uid;
    let slide0 = app.session.active().unwrap().current_slide().unwrap().id;
    let id = app.begin_external_file("drop");
    app.session.execute("slide.new", &json!({})).unwrap();
    let slide1 = app.session.active().unwrap().current_slide().unwrap().id;
    deliver(&inbox, id, "shot.png", png());
    app.drain_inbox();
    assert_eq!(app.session.active().unwrap().current_slide().unwrap().id, slide1, "the view stays on the slide the user moved to");
    let status = app.status.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
    assert_eq!(pictures(&app, uid, slide0), 1, "{status}");
    assert_eq!(pictures(&app, uid, slide1), 0);
}

/// Insert ▸ 3D Models: the picked file becomes a 3-D model; a dropped STL is recognised as one.
#[test]
fn picked_and_dropped_models_become_3d_models() {
    const STL: &[u8] = b"solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 0 1\nendloop\nendfacet\nendsolid t\n";
    let (mut app, inbox, started) = setup();
    app.run("app.insertModel3dDialog", json!({})).expect("dialog");
    let (purpose, id) = started.lock().expect("request log")[0].clone();
    assert_eq!(purpose, "model3d");
    deliver(&inbox, id, "part.stl", STL.to_vec());
    app.drain_inbox();
    let models = |app: &SlideApp| {
        app.session.active().unwrap().current_slide().unwrap().shapes.iter().filter(|s| matches!(s.kind, ShapeKind::Model3d { .. })).count()
    };
    assert_eq!(models(&app), 1);
    assert_eq!(super::infer_command("part.stl", STL), Some("insert.model3d"));
    assert_eq!(super::infer_command("scene.fbx", b"; FBX 7.4.0 project file"), Some("insert.model3d"));
    // A picture is still a picture.
    assert_eq!(super::infer_command("a.png", &png()), Some("insert.picture"));
}
