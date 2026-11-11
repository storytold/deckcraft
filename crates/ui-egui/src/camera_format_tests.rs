//! Camera Format ▸ Camera ▸ Preview: the split button and the More Options menu over a fake
//! camera source. CI has no camera, so every device here is scripted.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use deckcraft_engine::Session;
use deckcraft_media::camera::{CameraDevice, CameraSource, CameraStatus};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

/// What the fake source was asked to do, readable after it moved into the app.
#[derive(Default)]
struct Log {
    started: Vec<Option<String>>,
    stopped: usize,
    /// Statuses `start` returns for each call, then `fallback`.
    script: VecDeque<CameraStatus>,
    status: CameraStatus,
}

struct Fake {
    log: Arc<Mutex<Log>>,
    devices: Vec<CameraDevice>,
    start_status: CameraStatus,
    poll_status: CameraStatus,
}

impl Fake {
    fn with_devices(devices: Vec<CameraDevice>) -> (Box<dyn CameraSource>, Arc<Mutex<Log>>) {
        let log = Arc::new(Mutex::new(Log::default()));
        let fake = Fake { log: log.clone(), devices, start_status: CameraStatus::Requesting, poll_status: CameraStatus::Live };
        (Box::new(fake), log)
    }

    fn failing(status: CameraStatus) -> (Box<dyn CameraSource>, Arc<Mutex<Log>>) {
        let log = Arc::new(Mutex::new(Log::default()));
        let fake = Fake {
            log: log.clone(),
            devices: vec![CameraDevice { id: "front".into(), name: "Front Camera".into(), default: true }],
            start_status: status,
            poll_status: status,
        };
        (Box::new(fake), log)
    }

    fn without_devices(status: CameraStatus) -> (Box<dyn CameraSource>, Arc<Mutex<Log>>) {
        let log = Arc::new(Mutex::new(Log::default()));
        let fake = Fake { log: log.clone(), devices: Vec::new(), start_status: status, poll_status: status };
        (Box::new(fake), log)
    }
}

impl CameraSource for Fake {
    fn devices(&self) -> Vec<CameraDevice> {
        self.devices.clone()
    }

    fn start(&mut self, device: Option<&str>) -> CameraStatus {
        let mut log = self.log.lock().unwrap();
        log.started.push(device.map(str::to_string));
        let status = log.script.pop_front().unwrap_or(self.start_status);
        log.status = status;
        status
    }

    fn stop(&mut self) {
        let mut log = self.log.lock().unwrap();
        log.stopped += 1;
        log.status = CameraStatus::Off;
    }

    fn poll(&mut self) -> CameraStatus {
        let mut log = self.log.lock().unwrap();
        if log.status.is_on() {
            log.status = self.poll_status;
        }
        log.status
    }
}

fn devices() -> Vec<CameraDevice> {
    vec![
        CameraDevice { id: "front".into(), name: "Front Camera".into(), default: true },
        CameraDevice { id: "studio".into(), name: "Studio Camera".into(), default: false },
    ]
}

/// A deck with one cameo selected, on the Camera Format tab, over `source`.
fn app(source: Box<dyn CameraSource>) -> Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    s.execute("insert.cameo", &json!({})).expect("cameo");
    let mut app = SlideApp::new(s, Services { camera: Some(source), ..Default::default() });
    app.ui.tab = "Camera Format".into();
    let mut h = Harness::builder().with_size(egui::vec2(1280.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

#[test]
fn the_preview_button_starts_then_stops_the_source() {
    let (source, log) = Fake::with_devices(devices());
    let mut h = app(source);
    assert_eq!(h.state().camera_preview.status(), CameraStatus::Off);
    assert_eq!(h.state().session.camera_status, CameraStatus::Off);

    h.get_by_label("Enable Camera Preview").click();
    h.run_steps(2);
    {
        let log = log.lock().unwrap();
        assert_eq!(log.started, [None], "the default camera was asked for");
    }
    assert_eq!(h.state().camera_preview.status(), CameraStatus::Live, "the source's first status arrived");
    assert_eq!(h.state().session.camera_status, CameraStatus::Live);
    assert_eq!(h.state_mut().session.execute("cameo.preview.state", &json!({})).unwrap()["state"], "live");

    h.get_by_label("Enable Camera Preview").click();
    h.run_steps(2);
    assert_eq!(log.lock().unwrap().stopped, 1);
    assert_eq!(h.state().camera_preview.status(), CameraStatus::Off);
    assert_eq!(h.state().session.camera_status, CameraStatus::Off, "the session reports the stop too");
}

#[test]
fn the_preview_is_editor_state_not_a_document_edit() {
    let (source, _) = Fake::with_devices(devices());
    let mut h = app(source);
    let (revision, undo, dirty) = {
        let doc = h.state().session.active().unwrap();
        (doc.revision, doc.history.undo.len(), doc.is_dirty())
    };
    h.get_by_label("Enable Camera Preview").click();
    h.run_steps(2);
    h.get_by_label("Enable Camera Preview").click();
    h.run_steps(2);
    let doc = h.state().session.active().unwrap();
    assert_eq!((doc.revision, doc.history.undo.len(), doc.is_dirty()), (revision, undo, dirty));
}

#[test]
fn a_refused_or_missing_camera_keeps_the_editor_usable_and_says_why() {
    for status in [CameraStatus::Denied, CameraStatus::NoDevice, CameraStatus::Busy, CameraStatus::Unsupported] {
        let (source, log) = Fake::failing(status);
        let mut h = app(source);
        h.get_by_label("Enable Camera Preview").click();
        h.run_steps(2);
        assert_eq!(h.state().camera_preview.status(), status, "{status:?}");
        assert_eq!(h.state().session.camera_status, status, "{status:?}");
        assert!(!h.state().camera_preview.is_on(), "{status:?} is not a running preview");
        let message = h.state_mut().fresh_status().expect("the status bar explains the failure");
        assert!(message.to_lowercase().contains("camera"), "{status:?}: {message}");

        // The button stays live: a second click retries the source instead of doing nothing.
        h.get_by_label("Enable Camera Preview").click();
        h.run_steps(2);
        assert_eq!(log.lock().unwrap().started.len(), 2, "the retry reached the source");
        assert_eq!(h.state().camera_preview.status(), status);
    }
}

#[test]
fn an_unplugged_camera_turns_live_into_lost_without_stopping_the_editor() {
    let (source, log) = Fake::with_devices(devices());
    let mut h = app(source);
    h.get_by_label("Enable Camera Preview").click();
    h.run_steps(2);
    assert_eq!(h.state().camera_preview.status(), CameraStatus::Live);

    // The device goes away between two frames.
    log.lock().unwrap().status = CameraStatus::Lost;
    h.run_steps(2);
    assert_eq!(h.state().camera_preview.status(), CameraStatus::Lost);
    assert_eq!(h.state().session.camera_status, CameraStatus::Lost);
    assert!(!h.state().camera_preview.is_on());
    let message = h.state_mut().fresh_status().expect("the loss is announced");
    assert!(message.contains("disconnected"), "{message}");
    assert!(h.state().session.active().is_some(), "the presentation stays open");
}

#[test]
fn more_options_lists_the_cameras_and_picks_one() {
    let (source, log) = Fake::with_devices(devices());
    let mut h = app(source);
    h.get_by_label("More Options").click();
    h.run_steps(2);
    assert!(h.query_by_label("Turn On Camera Preview").is_some(), "the menu carries the toggle preview item");
    assert!(h.query_by_label("Front Camera").is_some());
    assert!(h.query_by_label("Studio Camera").is_some());

    h.get_by_label("Studio Camera").click();
    h.run_steps(2);
    let log = log.lock().unwrap();
    assert_eq!(log.started, [Some("studio".to_string())], "the picked camera is the one opened");
    drop(log);
    assert_eq!(h.state().session.camera_device.as_deref(), Some("studio"));
    assert_eq!(h.state().camera_preview.status(), CameraStatus::Live);
    assert!(h.query_by_label("Studio Camera").is_none(), "the menu closes after a pick");
}

#[test]
fn the_split_arrow_opens_the_same_menu_and_its_toggle_stops_the_preview() {
    let (source, log) = Fake::with_devices(devices());
    let mut h = app(source);
    h.get_by_label("Enable Camera Preview").click();
    h.run_steps(2);

    h.get_by_label("Enable Camera Preview options").click();
    h.run_steps(2);
    assert!(h.query_by_label("Turn Off Camera Preview").is_some());
    // The picked default camera is ticked in the list.
    assert!(h.query_by_label("Front Camera").is_some());

    h.get_by_label("Turn Off Camera Preview").click();
    h.run_steps(2);
    assert_eq!(log.lock().unwrap().stopped, 1);
    assert_eq!(h.state().camera_preview.status(), CameraStatus::Off);
    assert_eq!(h.state().session.camera_status, CameraStatus::Off);
}

#[test]
fn a_build_without_capture_says_so_instead_of_listing_devices() {
    let (source, _) = Fake::without_devices(CameraStatus::Unsupported);
    let mut h = app(source);
    // The failure is what tells the menu the build has no capture, so ask for a preview first.
    h.get_by_label("Enable Camera Preview").click();
    h.run_steps(2);
    assert_eq!(h.state().camera_preview.status(), CameraStatus::Unsupported);
    h.get_by_label("More Options").click();
    h.run_steps(2);
    assert!(h.query_by_label("Camera preview is not available in this build").is_some());
    assert!(h.query_by_label("Front Camera").is_none());
}
