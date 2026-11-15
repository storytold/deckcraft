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

/// #2040: on the desktop, Insert Picture opens a dialog that answers later instead of blocking
/// the window. Nothing changes while it is open, a second dialog waits its turn, and the picture
/// lands on the slide the dialog was opened from.
#[test]
fn a_picture_from_a_nonblocking_dialog_lands_on_the_slide_that_asked() {
    type Answer = std::sync::mpsc::Sender<Option<String>>;
    let dialogs: Arc<Mutex<Vec<(String, Answer)>>> = Arc::default();
    let blocking = Arc::new(Mutex::new(0));
    let (record, blocked) = (dialogs.clone(), blocking.clone());
    let services = Services {
        pick_open: Some(Box::new(move |_: &str| {
            *blocked.lock().unwrap() += 1;
            None
        })),
        open_dialog: Some(Box::new(move |purpose: &str| {
            let (answer, rx) = std::sync::mpsc::channel();
            record.lock().unwrap().push((purpose.to_string(), answer));
            rx
        })),
        read: Some(Box::new(|_: &str| Ok(png()))),
        ..Default::default()
    };
    let mut app = SlideApp::new(Session::with_new(), services);
    let uid = app.session.active().unwrap().uid;
    let slide0 = app.session.active().unwrap().current_slide().unwrap().id;
    app.session.execute("slide.new", &json!({})).unwrap();
    let slide1 = app.session.active().unwrap().current_slide().unwrap().id;
    app.session.execute("slide.go", &json!({"id": slide0.0})).unwrap();

    app.run("app.insertPictureDialog", json!({})).unwrap();
    app.poll_pick();
    assert_eq!(pictures(&app, uid, slide0), 0, "nothing is inserted before the dialog answers");
    app.run("app.insertAudioDialog", json!({})).unwrap();
    assert_eq!(dialogs.lock().unwrap().len(), 1, "one dialog at a time");
    assert!(app.status.as_ref().is_some_and(|(t, _)| t.contains("already open")));

    // The user moves on before choosing the file.
    app.session.execute("slide.go", &json!({"id": slide1.0})).unwrap();
    let (purpose, answer) = dialogs.lock().unwrap().remove(0);
    assert_eq!(purpose, "picture");
    answer.send(Some("/photos/shot.png".into())).unwrap();
    app.poll_pick();
    let status = app.status.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
    assert_eq!(pictures(&app, uid, slide0), 1, "{status}");
    assert_eq!(pictures(&app, uid, slide1), 0);
    assert_eq!(app.session.active().unwrap().current_slide().unwrap().id, slide1, "the view stays where the user went");

    // A dialog closed without a file inserts nothing, and the next one may open.
    app.run("app.insertPictureDialog", json!({})).unwrap();
    drop(dialogs.lock().unwrap().remove(0));
    app.poll_pick();
    assert_eq!(pictures(&app, uid, slide1), 0);
    app.run("app.insertPictureDialog", json!({})).unwrap();
    assert_eq!(dialogs.lock().unwrap().len(), 1);
    assert_eq!(*blocking.lock().unwrap(), 0, "the blocking picker is never used");
}

type Dialogs = Arc<Mutex<Vec<(String, std::sync::mpsc::Sender<Option<String>>)>>>;

/// A desktop app whose open dialogs wait in `Dialogs` until the test answers them; `file` is
/// what reading the picked path gives.
fn nonblocking_app(file: Vec<u8>) -> (SlideApp, Dialogs) {
    let dialogs: Dialogs = Arc::default();
    let record = dialogs.clone();
    let services = Services {
        pick_open: Some(Box::new(|_: &str| panic!("the blocking picker is never used"))),
        open_dialog: Some(Box::new(move |purpose: &str| {
            let (answer, rx) = std::sync::mpsc::channel();
            record.lock().unwrap().push((purpose.to_string(), answer));
            rx
        })),
        read: Some(Box::new(move |_: &str| Ok(file.clone()))),
        ..Default::default()
    };
    (SlideApp::new(Session::with_new(), services), dialogs)
}

fn answer(app: &mut SlideApp, dialogs: &Dialogs, path: &str) {
    let (_, answer) = dialogs.lock().unwrap().remove(0);
    answer.send(Some(path.into())).unwrap();
    app.poll_pick();
}

/// #2040: Change Picture replaces the picture selected when the dialog opened, even if another
/// picture is selected, or another slide shown, before the file is chosen.
#[test]
fn change_picture_replaces_the_picture_selected_when_the_dialog_opened() {
    let svg = b"<svg xmlns='http://www.w3.org/2000/svg' width='4' height='4'><rect width='4' height='4'/></svg>".to_vec();
    let media_of = |app: &SlideApp, id: u64| {
        let d = app.session.active().unwrap();
        d.doc.slides[0]
            .shapes
            .iter()
            .find_map(|s| match &s.kind {
                ShapeKind::Picture { fill } if u64::from(s.id.0) == id => Some(fill.media),
                _ => None,
            })
            .expect("picture")
    };
    for move_to_another_slide in [false, true] {
        let (mut app, dialogs) = nonblocking_app(svg.clone());
        let data = deckcraft_engine::cmd::base64_encode(&png());
        let a = app.session.execute("insert.picture", &json!({"name": "a.png", "data": data})).unwrap()["id"].as_u64().unwrap();
        let b = app.session.execute("insert.picture", &json!({"name": "b.png", "data": data})).unwrap()["id"].as_u64().unwrap();
        let (a_before, b_before) = (media_of(&app, a), media_of(&app, b));
        app.session.execute("edit.select", &json!({"ids": [a]})).unwrap();
        app.run("app.changePictureDialog", json!({})).unwrap();
        app.session.execute("edit.select", &json!({"ids": [b]})).unwrap();
        if move_to_another_slide {
            app.session.execute("slide.new", &json!({})).unwrap();
        }
        answer(&mut app, &dialogs, "/photos/new.svg");
        if move_to_another_slide {
            app.session.execute("slide.go", &json!({"index": 0})).unwrap();
        }
        let status = app.status.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
        assert_ne!(media_of(&app, a), a_before, "the picture selected when the dialog opened is replaced: {status}");
        assert_eq!(media_of(&app, b), b_before, "the picture selected later is left alone");
    }
}

/// #2040: a transition sound goes to the slides selected when the dialog opened, even if the
/// slide selection changes (on the same current slide, or to another slide) before it is chosen.
#[test]
fn a_transition_sound_goes_to_the_slides_selected_when_the_dialog_opened() {
    let sounds = |app: &SlideApp| -> Vec<bool> {
        app.session.active().unwrap().doc.slides.iter().map(|s| s.transition.as_ref().is_some_and(|t| t.sound.is_some())).collect()
    };
    for later in [&[0usize][..], &[2]] {
        let (mut app, dialogs) = nonblocking_app(wav());
        app.session.execute("slide.new", &json!({})).unwrap();
        app.session.execute("slide.new", &json!({})).unwrap();
        let ids: Vec<_> = app.session.active().unwrap().doc.slides.iter().map(|s| s.id).collect();
        app.session.execute("slide.go", &json!({"index": 0})).unwrap();
        app.session.active_mut().unwrap().selection.slides = vec![ids[0], ids[1]];
        app.pick_transition_sound();
        // The user selects other slides before choosing the sound.
        app.session.execute("slide.go", &json!({"index": later[0]})).unwrap();
        app.session.active_mut().unwrap().selection.slides = later.iter().map(|&i| ids[i]).collect();
        answer(&mut app, &dialogs, "/sounds/chime.wav");
        let status = app.status.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
        assert_eq!(sounds(&app), [true, true, false], "selected later: {later:?}; {status}");
        let st = app.session.active().unwrap();
        assert_eq!(st.selection.slide, later[0], "the view stays where the user went");
        assert_eq!(st.selection.slides, later.iter().map(|&i| ids[i]).collect::<Vec<_>>(), "and so does the selection");
    }
}

/// #2040: Reuse Slides uses the non-blocking dialog too; the slides go after the slide that
/// was current when it opened.
#[test]
fn reuse_slides_uses_the_nonblocking_dialog() {
    let mut src = Session::with_new();
    src.execute("slide.new", &json!({"layout": "titleAndContent", "title": "From elsewhere"})).unwrap();
    let bytes = deckcraft_engine::cmd::file::save_bytes(&src.doc().unwrap().doc, "deckcraft").unwrap();
    let (mut app, dialogs) = nonblocking_app(bytes);
    let first = app.session.active().unwrap().current_slide().unwrap().id;
    app.session.execute("slide.new", &json!({})).unwrap();
    app.session.execute("slide.go", &json!({"index": 0})).unwrap();
    app.run("app.reuseSlidesDialog", json!({})).unwrap();
    assert!(app.dialog.is_none(), "nothing is listed before the dialog answers");
    app.session.execute("slide.go", &json!({"index": 1})).unwrap();
    answer(&mut app, &dialogs, "/decks/src.deckcraft");
    assert_eq!(app.dialog.as_ref().map(|d| d.id.as_str()), Some("reuseSlides"));
    assert!(app.reuse_slides(&app.dialog.as_ref().unwrap().params["source"].clone(), false, &[1]));
    let st = app.session.active().unwrap();
    assert_eq!(st.doc.slides.len(), 3);
    assert_eq!(st.doc.slides[0].id, first);
    assert_eq!(st.doc.slides[1].title(), "From elsewhere", "inserted after the slide current when the dialog opened");
}
