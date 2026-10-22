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

fn models(app: &SlideApp, doc: u64, slide: deckcraft_model::SlideId) -> Vec<deckcraft_model::Model3d> {
    let st = app.session.documents().iter().find(|d| d.uid == doc).expect("document");
    let i = st.doc.slide_index(slide).expect("slide");
    st.doc.slides[i]
        .shapes
        .iter()
        .filter_map(|s| match &s.kind {
            ShapeKind::Model3d(m) => Some((**m).clone()),
            _ => None,
        })
        .collect()
}

/// A 3-D model picked with the files it names comes back as one request: one model, on the
/// slide that asked, wearing its texture; the material and texture files are not inserted.
#[test]
fn a_model_picked_with_its_companions_is_one_model_on_the_slide_that_asked() {
    let (mut app, inbox, started) = setup();
    let uid = app.session.active().unwrap().uid;
    let slide0 = app.session.active().unwrap().current_slide().unwrap().id;
    app.pick_and_insert("model3d", "insert.model3d");
    let (purpose, id) = started.lock().expect("request log").last().cloned().unwrap();
    assert_eq!(purpose, "model3d");
    app.session.execute("slide.new", &json!({})).unwrap();
    let slide1 = app.session.active().unwrap().current_slide().unwrap().id;

    let obj = b"mtllib tri.mtl\nusemtl skin\nv 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nf 1/1 2/2 3/3\n".to_vec();
    let mtl = b"newmtl skin\nKd 1 1 1\nmap_Kd skin.png\n".to_vec();
    let files = vec![("tri.obj".to_string(), obj), ("tri.mtl".to_string(), mtl), ("skin.png".to_string(), png())];
    inbox.lock().expect("inbox").push(InboxEvent::Files { id, files });
    app.drain_inbox();

    let status = app.status.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
    let placed = models(&app, uid, slide0);
    assert_eq!(placed.len(), 1, "{status}");
    assert!(models(&app, uid, slide1).is_empty());
    assert_eq!(pictures(&app, uid, slide0) + pictures(&app, uid, slide1), 0, "the texture belongs to the model");
    let st = app.session.documents().iter().find(|d| d.uid == uid).unwrap();
    let glb = st.doc.media(placed[0].media).expect("model part").data.clone();
    let scene = deckcraft_mesh::load("tri.glb", &glb, &deckcraft_mesh::no_files).expect("glb");
    assert_eq!(scene.textures.len(), 1, "the material file was read with the model");
}

/// A one-triangle GLB whose positions are in the external `tri.bin`, and that buffer.
fn glb_with_external_buffer() -> (Vec<u8>, Vec<u8>) {
    let mut bin = Vec::new();
    for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let mut js = json!({
        "asset": {"version": "2.0"},
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
        "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}],
        "bufferViews": [{"buffer": 0, "byteLength": 36}],
        "buffers": [{"byteLength": 36, "uri": "tri.bin"}],
    })
    .to_string()
    .into_bytes();
    while !js.len().is_multiple_of(4) {
        js.push(b' ');
    }
    let mut glb = b"glTF".to_vec();
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&((20 + js.len()) as u32).to_le_bytes());
    glb.extend_from_slice(&(js.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    glb.extend_from_slice(&js);
    (glb, bin)
}

/// A GLB picked with the buffer it names gets it; a GLB that holds everything, dropped with a
/// photo, leaves the photo a picture of its own.
#[test]
fn a_glb_takes_its_buffer_but_not_unrelated_pictures() {
    let (mut app, inbox, started) = setup();
    let uid = app.session.active().unwrap().uid;
    let slide = app.session.active().unwrap().current_slide().unwrap().id;
    app.pick_and_insert("model3d", "insert.model3d");
    let (_, id) = started.lock().expect("request log").last().cloned().unwrap();
    let (glb, bin) = glb_with_external_buffer();
    inbox.lock().expect("inbox").push(InboxEvent::Files { id, files: vec![("tri.glb".into(), glb), ("tri.bin".into(), bin)] });
    app.drain_inbox();
    let status = app.status.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
    assert_eq!(models(&app, uid, slide).len(), 1, "{status}");

    let mut p = deckcraft_mesh::Primitive { material: None, ..Default::default() };
    p.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    p.indices = vec![0, 1, 2];
    let whole = deckcraft_mesh::to_glb(&deckcraft_mesh::Scene { primitives: vec![p], ..Default::default() });
    app.open_or_insert_files(vec![("whole.glb".into(), whole, None), ("photo.png".into(), png(), None)]);
    let status = app.status.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
    assert_eq!(models(&app, uid, slide).len(), 2, "{status}");
    assert_eq!(pictures(&app, uid, slide), 1, "{status}");
}

/// An OBJ dropped with its material, its texture and an unrelated photo: one textured model,
/// and the photo is still a picture; the material file is quietly the model's.
#[test]
fn an_obj_keeps_its_files_and_leaves_an_unrelated_photo_a_picture() {
    let (mut app, _inbox, _started) = setup();
    let uid = app.session.active().unwrap().uid;
    let slide = app.session.active().unwrap().current_slide().unwrap().id;
    let obj = b"mtllib tri.mtl\nusemtl skin\nv 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nf 1/1 2/2 3/3\n".to_vec();
    let mtl = b"newmtl skin\nKd 1 1 1\nmap_Kd skin.png\n".to_vec();
    app.open_or_insert_files(vec![
        ("tri.obj".into(), obj, None),
        ("tri.mtl".into(), mtl, None),
        ("skin.png".into(), png(), None),
        ("holiday.png".into(), png(), None),
    ]);
    let status = app.status.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
    let placed = models(&app, uid, slide);
    assert_eq!(placed.len(), 1, "{status}");
    assert_eq!(pictures(&app, uid, slide), 1, "the unrelated photo: {status}");
    assert!(!status.contains("can't open"), "{status}");
    let st = app.session.documents().iter().find(|d| d.uid == uid).unwrap();
    let glb = st.doc.media(placed[0].media).expect("model part").data.clone();
    assert_eq!(deckcraft_mesh::load("tri.glb", &glb, &deckcraft_mesh::no_files).expect("glb").textures.len(), 1);
}
