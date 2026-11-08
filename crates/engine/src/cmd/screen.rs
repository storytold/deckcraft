//! Screen recording (`show.screenRecord`): the Record tab and the Record window drive it.
//!
//! The command owns the state machine (start, pause, resume, stop, cancel) so the UI, the control
//! channel and MCP all do exactly the same thing. Frames flow through the session's
//! [`ScreenState`](crate::screen::ScreenState), which the UI ticks; stopping inserts the finished
//! MP4 as an ordinary video shape on the current slide, with a poster frame, through
//! `insert.video`. The temporary file is removed once the bytes are in the presentation.

use deckcraft_screen::{CaptureOptions, CaptureRegion};
use serde_json::{Value, json};

use super::{CommandSpec, always, bad, bool_or, cmd, has_slide, str_param, usize_param};
use crate::Session;
use crate::screen::ScreenState;

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "show.screenRecord",
        "Screen Recording",
        ["Record", "Record"],
        None,
        "{action?: start|pause|resume|stop|cancel|targets|permission, target?: id, targetKind?: display|window, region?: [x,y,w,h] px, fps?: 1-60, cursor?: bool, quality?: 10-51} → {state, …} (`stop` answers {state: stopping}; the take is inserted on a later tick, once the encoder has written it)",
        always,
        run
    )]
}

const CMD: &str = "show.screenRecord";

fn err(e: deckcraft_screen::ScreenError) -> crate::EngineError {
    crate::EngineError::Other(e.to_string())
}

fn region(p: &Value) -> Result<Option<CaptureRegion>, crate::EngineError> {
    let Some(v) = p.get("region").filter(|v| !v.is_null()) else { return Ok(None) };
    let a = v.as_array().filter(|a| a.len() == 4).ok_or_else(|| bad(CMD, "`region` must be [x, y, width, height] in captured pixels"))?;
    let n = a.iter().map(|v| v.as_f64().filter(|v| v.is_finite())).collect::<Option<Vec<f64>>>();
    let n = n.ok_or_else(|| bad(CMD, "`region` must be four numbers"))?;
    let whole = |v: f64| v.fract() == 0.0;
    let int = |v: f64, lo: f64, hi: f64| whole(v) && (lo..=hi).contains(&v);
    if !int(n[0], f64::from(i32::MIN), f64::from(i32::MAX)) || !int(n[1], f64::from(i32::MIN), f64::from(i32::MAX)) {
        return Err(bad(CMD, "`region` has an origin outside its pixel range"));
    }
    if !int(n[2], 0.0, f64::from(u32::MAX)) || !int(n[3], 0.0, f64::from(u32::MAX)) {
        return Err(bad(CMD, "`region` has a size outside its pixel range"));
    }
    CaptureRegion::new(n[0] as i32, n[1] as i32, n[2] as u32, n[3] as u32).map(Some).map_err(|e| bad(CMD, e.to_string()))
}

fn fps(p: &Value) -> Result<u32, crate::EngineError> {
    let n = usize_param(p, "fps").unwrap_or(30);
    u32::try_from(n).ok().filter(|n| (1..=60).contains(n)).ok_or_else(|| bad(CMD, "`fps` must be from 1 to 60 frames per second"))
}

fn quality(p: &Value) -> Result<u8, crate::EngineError> {
    let n = usize_param(p, "quality").unwrap_or(24);
    u8::try_from(n).ok().filter(|n| (10..=51).contains(n)).ok_or_else(|| bad(CMD, "`quality` must be from 10 (best) to 51 (smallest)"))
}

/// The capture target the caller named: `Ok(None)` when absent (a host that picks the source
/// itself, and the primary display everywhere else), `Err` when present but not a target id, so a
/// malformed id never starts a capture of the default surface instead.
fn target(p: &Value) -> Result<Option<u64>, crate::EngineError> {
    // Id 0 is a real `scap` target: only an absent target means the primary display.
    match p.get("target") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| bad(CMD, format!("`target` is not a capture target id: {v}"))),
    }
}

fn target_kind(p: &Value) -> Result<Option<deckcraft_screen::TargetKind>, crate::EngineError> {
    match str_param(p, "targetKind").map(|k| k.trim().to_ascii_lowercase()) {
        None => Ok(None),
        Some(k) if k.is_empty() => Ok(None),
        Some(k) if k == "display" => Ok(Some(deckcraft_screen::TargetKind::Display)),
        Some(k) if k == "window" => Ok(Some(deckcraft_screen::TargetKind::Window)),
        Some(other) => Err(bad(CMD, format!("`targetKind` “{other}” isn't display or window"))),
    }
}

fn state_json(s: &ScreenState) -> Value {
    let state = if s.is_finishing() {
        // Stop has asked for the file; the take is neither recording nor paused any more.
        "stopping"
    } else if s.is_paused() {
        "paused"
    } else {
        "recording"
    };
    json!({
        "state": state,
        "frames": s.frames(),
        "durationMs": s.elapsed_ms(),
    })
}

fn run(s: &mut Session, p: &Value) -> crate::Result<Value> {
    let action = str_param(p, "action").unwrap_or("start").trim().to_ascii_lowercase();
    match action.as_str() {
        "start" => {
            // Every other action only drives the recorder: a document closed (or all its slides
            // deleted) must still be able to stop the capture and keep the take.
            has_slide(s).map_err(|e| bad(CMD, e))?;
            let fps = fps(p)?;
            let region = region(p)?;
            let show_cursor = bool_or(p, "cursor", false);
            let options = CaptureOptions { target: target(p)?, kind: target_kind(p)?, region, fps, show_cursor };
            let mut cfg = deckcraft_screen::RecorderConfig::new(fps);
            cfg.region = region;
            cfg.quality = quality(p)?;
            s.screen.start(options, cfg).map_err(err)?;
            Ok(json!({
                "state": "recording",
                "fps": fps,
                "region": region.map(CaptureRegion::to_array),
                "cursor": show_cursor,
            }))
        }
        "pause" => {
            if s.screen.is_finishing() {
                return Err(bad(CMD, "the recording is being written"));
            }
            s.screen.pause().map_err(err)?;
            Ok(state_json(&s.screen))
        }
        "resume" => {
            if s.screen.is_finishing() {
                return Err(bad(CMD, "the recording is being written"));
            }
            s.screen.resume().map_err(err)?;
            Ok(state_json(&s.screen))
        }
        "cancel" => {
            let was = s.screen.cancel();
            Ok(json!({"state": if was { "cancelled" } else { "idle" }}))
        }
        "stop" => {
            // The encoder writes the file on its thread; the take is inserted on the tick that
            // receives it, so Stop answers at once instead of freezing the UI on encoding.
            s.screen.stop().map_err(err)?;
            Ok(json!({"state": "stopping"}))
        }
        "targets" => {
            // A missing permission or an unsupported host is the picker's state, not a failed
            // request: report it with an empty list so the "Grant permission…" button can appear.
            let targets = match s.screen.targets() {
                Ok(t) => t,
                Err(
                    deckcraft_screen::ScreenError::PermissionDenied
                    | deckcraft_screen::ScreenError::Unavailable
                    | deckcraft_screen::ScreenError::NoDevice,
                ) => Vec::new(),
                Err(e) => return Err(err(e)),
            };
            let list: Vec<Value> = targets
                .iter()
                .map(|t| json!({"id": t.id, "kind": t.kind.as_str(), "title": t.title, "width": t.width, "height": t.height}))
                .collect();
            // Some hosts pick the source themselves (Linux, where `scap` returns no targets and
            // chooses when the recorder is built): the picker may start the take with no target.
            let implicit = list.is_empty() && s.screen.status() == deckcraft_screen::CaptureStatus::Ready;
            Ok(json!({"targets": list, "status": status_str(s.screen.status()), "implicit": implicit}))
        }
        "permission" => {
            let granted = s.screen.request_permission();
            Ok(json!({"granted": granted, "status": status_str(s.screen.status())}))
        }
        other => Err(bad(CMD, format!("action “{other}” isn't start, pause, resume, stop, cancel, targets or permission"))),
    }
}

/// Insert a finished take on the current slide and delete its temporary file. Both the `stop`
/// action and a recording that a capture failure ended use this, so the shape is the same.
pub(crate) fn insert_take(s: &mut Session, stopped: &crate::screen::StoppedRecording) -> crate::Result<Value> {
    // Hide the temporary file name from the deck: the shape is named "Screen Recording". The file
    // is the recorder's own (not a caller's path), so it is read outside the automation roots a
    // command may be confined to; a failed insert keeps the take instead of deleting it.
    let path = stopped.path.to_string_lossy().to_string();
    // A failed insert keeps the take; the caller's message names the kept file (see
    // `Session::screen_tick`), not a second message here that would replace that path.
    let inserted = crate::file_access::unconfined(|| s.execute("insert.video", &json!({"path": path, "name": "Screen Recording.mp4"})))?;
    let _ = std::fs::remove_file(&stopped.path);
    // Tag the shape so Clear Recording can remove takes without touching ordinary videos.
    if let Some(id) = inserted.get("id").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()) {
        let id = deckcraft_model::ShapeId(id);
        let _ = s.edit(|doc, sel| {
            if let Some(slide) = doc.slides.get_mut(sel.slide) {
                let slide = std::sync::Arc::make_mut(slide);
                if let Some(shape) = slide.shape_mut(id) {
                    shape.tags.push(("deckcraft.recording".into(), "media".into()));
                }
            }
            Ok(())
        });
    }
    Ok(inserted)
}

fn status_str(status: deckcraft_screen::CaptureStatus) -> &'static str {
    match status {
        deckcraft_screen::CaptureStatus::Ready => "ready",
        deckcraft_screen::CaptureStatus::Unsupported => "unavailable",
        deckcraft_screen::CaptureStatus::PermissionDenied => "permissionDenied",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picker_region_must_be_a_four_number_array_of_at_least_64_by_64() {
        assert_eq!(region(&json!({"region": [10, 20, 640, 480]})).expect("valid"), Some(CaptureRegion { x: 10, y: 20, width: 640, height: 480 }));
        assert!(region(&json!({"region": [10, 20, 63, 480]})).is_err());
        assert!(region(&json!({"region": [10, 20, 640]})).is_err());
        assert!(region(&json!({"region": [-1, 0, 640, 480]})).is_err());
        assert_eq!(region(&json!({})).expect("absent"), None);
    }

    /// A host that lists no targets but can record (Linux: `scap` chooses the source itself) is
    /// startable: the picker must not disable Record for a missing list.
    #[test]
    fn an_empty_target_list_on_a_ready_host_is_startable() {
        struct SelfPicking;
        impl deckcraft_screen::ScreenIn for SelfPicking {
            fn status(&mut self) -> deckcraft_screen::CaptureStatus {
                deckcraft_screen::CaptureStatus::Ready
            }
            fn targets(&mut self) -> Result<Vec<deckcraft_screen::CaptureTarget>, deckcraft_screen::ScreenError> {
                Err(deckcraft_screen::ScreenError::NoDevice)
            }
            fn start(&mut self, _o: &CaptureOptions) -> Result<(), deckcraft_screen::ScreenError> {
                Ok(())
            }
            fn frame(&mut self) -> Result<Option<deckcraft_screen::ScreenFrame>, deckcraft_screen::ScreenError> {
                Ok(None)
            }
            fn stop(&mut self) {}
        }
        let mut s = Session::with_new();
        s.screen.set_host(Some(Box::new(SelfPicking)));
        let v = s.execute("show.screenRecord", &json!({"action": "targets"})).unwrap();
        assert_eq!(v["targets"].as_array().map(Vec::len), Some(0));
        assert_eq!(v["status"], "ready");
        assert_eq!(v["implicit"], true, "the host picks the source itself: {v}");
        // Starting without a target is what the picker sends then, and the host accepts it.
        let started = s.execute("show.screenRecord", &json!({"action": "start"})).unwrap();
        assert!(started.get("state").is_some(), "{started}");
        let _ = s.screen.cancel();
    }

    #[test]
    fn rates_and_quality_are_bounded() {
        assert_eq!(fps(&json!({"fps": 30})).expect("fps"), 30);
        assert!(fps(&json!({"fps": 0})).is_err());
        assert!(fps(&json!({"fps": 61})).is_err());
        assert_eq!(quality(&json!({})).expect("default"), 24);
        assert!(quality(&json!({"quality": 9})).is_err());
        assert!(quality(&json!({"quality": 52})).is_err());
    }

    #[test]
    fn a_picked_target_kind_is_read_and_unknown_kinds_are_refused() {
        assert_eq!(target_kind(&json!({})).expect("absent"), None);
        assert_eq!(target_kind(&json!({"targetKind": "window"})).expect("window"), Some(deckcraft_screen::TargetKind::Window));
        assert_eq!(target_kind(&json!({"targetKind": "Display"})).expect("display"), Some(deckcraft_screen::TargetKind::Display));
        assert!(target_kind(&json!({"targetKind": "monitor"})).is_err(), "an unknown kind is refused");
    }

    #[test]
    fn a_target_that_is_not_an_id_is_refused_instead_of_recording_the_default_screen() {
        assert_eq!(target(&json!({})).expect("absent"), None);
        assert_eq!(target(&json!({"target": null})).expect("null is absent"), None);
        assert_eq!(target(&json!({"target": 0})).expect("id 0 is a real target"), Some(0));
        assert_eq!(target(&json!({"target": 7})).expect("a target id"), Some(7));
        for malformed in [json!({"target": -1}), json!({"target": "screen"}), json!({"target": 1.5}), json!({"target": true})] {
            let e = target(&malformed).expect_err("not a target id").to_string();
            assert!(e.contains("`target`"), "{malformed}: {e}");
        }
    }

    #[test]
    fn stopping_and_cancelling_need_no_slide() {
        // Closing the document (or deleting every slide) must not strand a running capture: only
        // `start` needs a slide to insert the take on.
        let frames = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
        let mut s = Session::with_new();
        s.set_screen_host(Some(Box::new(FakeScreen { frames: frames.clone() })));
        s.execute(CMD, &json!({"fps": 30})).expect("start");
        frames.lock().expect("queue").push_back(deckcraft_screen::ScreenFrame::solid(160, 120, [1, 2, 3, 255]));
        s.screen_tick(0.0);
        // No document at all: the recorder still stops, though a start needs a slide to insert on.
        let mut empty = Session::new();
        empty.set_screen_host(Some(Box::new(FakeScreen { frames: std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())) })));
        assert!(empty.execute(CMD, &json!({"action": "start"})).is_err(), "start still needs a slide");
        empty.execute(CMD, &json!({"action": "cancel"})).expect("cancel needs no slide");
        let v = s.execute(CMD, &json!({"action": "stop"})).expect("stop with a slide");
        assert_eq!(v["state"], "stopping", "{v}");
        let slide = s.active().expect("doc").selection.slide;
        assert!(pump_until_inserted(&mut s, slide), "the finished take is inserted on a tick");
    }

    #[test]
    fn pause_and_resume_are_refused_while_the_take_is_written() {
        let frames = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
        let mut s = Session::with_new();
        s.set_screen_host(Some(Box::new(FakeScreen { frames: frames.clone() })));
        s.execute(CMD, &json!({"fps": 30})).expect("start");
        frames.lock().expect("queue").push_back(deckcraft_screen::ScreenFrame::solid(160, 120, [5, 6, 7, 255]));
        s.screen_tick(0.0);
        s.execute(CMD, &json!({"action": "stop"})).expect("stop");
        // The take is the encoder's now: pausing it would neither stop nor resume the write.
        for action in ["pause", "resume"] {
            let e = s.execute(CMD, &json!({"action": action})).unwrap_err().to_string();
            assert!(e.contains("being written"), "{action}: {e}");
        }
        assert_eq!(state_json(&s.screen)["state"], "stopping");
        let slide = s.active().expect("doc").selection.slide;
        assert!(pump_until_inserted(&mut s, slide), "the finished take is still inserted");
    }

    #[test]
    fn without_a_host_screen_recording_reports_unavailable() {
        let mut s = Session::with_new();
        let e = s.execute(CMD, &json!({"action": "start"})).unwrap_err().to_string();
        assert!(e.contains("isn't available"), "{e}");
        // The picker asks for targets first: an unavailable host is its status, with no screens,
        // rather than an error that would leave "Grant permission…" unreachable.
        let v = s.execute(CMD, &json!({"action": "targets"})).expect("targets reports the state");
        assert_eq!(v["status"], "unavailable", "{v}");
        assert_eq!(v["targets"].as_array().map(Vec::len), Some(0), "{v}");
    }

    /// A fake screen device with a frame queue the test fills.
    struct FakeScreen {
        frames: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<deckcraft_screen::ScreenFrame>>>,
    }

    impl deckcraft_screen::ScreenIn for FakeScreen {
        fn status(&mut self) -> deckcraft_screen::CaptureStatus {
            deckcraft_screen::CaptureStatus::Ready
        }
        fn targets(&mut self) -> Result<Vec<deckcraft_screen::CaptureTarget>, deckcraft_screen::ScreenError> {
            Ok(vec![deckcraft_screen::CaptureTarget {
                id: 1,
                kind: deckcraft_screen::TargetKind::Display,
                title: "Fake display".into(),
                width: 160,
                height: 120,
            }])
        }
        fn start(&mut self, _o: &CaptureOptions) -> Result<(), deckcraft_screen::ScreenError> {
            Ok(())
        }
        fn frame(&mut self) -> Result<Option<deckcraft_screen::ScreenFrame>, deckcraft_screen::ScreenError> {
            Ok(self.frames.lock().map_err(|_| deckcraft_screen::ScreenError::Failed("poisoned".into()))?.pop_front())
        }
        fn stop(&mut self) {}
    }

    /// Run ticks until the encoder hands the take over and the session inserts it (bounded), as
    /// the UI does: Stop itself only asks for the file.
    fn pump_until_inserted(s: &mut Session, slide: usize) -> bool {
        for _ in 0..500 {
            s.screen_tick(0.0);
            let done = s.active().is_some_and(|d| {
                d.doc.slides.get(slide).is_some_and(|sl| sl.shapes.iter().any(|sh| matches!(sh.kind, deckcraft_model::ShapeKind::Media(_))))
            });
            if done {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        false
    }

    #[test]
    fn stop_inserts_a_click_to_play_video_with_a_poster_and_undo_removes_it() {
        let frames = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
        let mut s = Session::with_new();
        s.set_screen_host(Some(Box::new(FakeScreen { frames: frames.clone() })));
        s.execute(CMD, &json!({"fps": 30})).expect("start");
        frames.lock().expect("queue").push_back(deckcraft_screen::ScreenFrame::solid(160, 120, [40, 90, 200, 255]));
        s.screen_tick(0.0);
        let r = s.execute(CMD, &json!({"action": "stop"})).expect("stop");
        assert_eq!(r["state"], "stopping", "Stop answers before the encoder writes the file: {r}");
        let slide = s.active().expect("doc").selection.slide;
        assert!(pump_until_inserted(&mut s, slide), "the finished take is inserted on a tick");
        let doc = s.active().expect("doc");
        let shape = doc
            .doc
            .slides
            .get(doc.selection.slide)
            .and_then(|sl| sl.shapes.iter().find(|sh| matches!(sh.kind, deckcraft_model::ShapeKind::Media(_))))
            .expect("the recording is on the slide");
        let deckcraft_model::ShapeKind::Media(clip) = &shape.kind else { panic!("media") };
        assert!(clip.video && !clip.autoplay, "a recording starts on click, not by itself");
        assert!(clip.poster.is_some(), "the first frame is the poster");
        assert!(clip.duration_ms > 0);
        assert!(shape.tags.iter().any(|(k, v)| k == "deckcraft.recording" && v == "media"));
        s.execute("edit.undo", &json!({})).expect("undo");
        let doc = s.active().expect("doc");
        assert!(!doc.doc.slides[doc.selection.slide].shapes.iter().any(|sh| matches!(sh.kind, deckcraft_model::ShapeKind::Media(_))));
    }

    #[test]
    fn stop_without_a_recording_is_a_plain_error_not_a_panic() {
        let mut s = Session::with_new();
        let e = s.execute(CMD, &json!({"action": "stop"})).unwrap_err().to_string();
        assert!(e.contains("no screen recording"), "{e}");
    }
}
