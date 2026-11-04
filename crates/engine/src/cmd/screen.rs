//! Screen recording (`show.screenRecord`): the Record tab and the Record window drive it.
//!
//! The command owns the state machine (start, pause, resume, stop, cancel) so the UI, the control
//! channel and MCP all do exactly the same thing. Frames flow through the session's
//! [`ScreenState`](crate::screen::ScreenState), which the UI ticks; stopping inserts the finished
//! MP4 as an ordinary video shape on the current slide, with a poster frame, through
//! `insert.video`. The temporary file is removed once the bytes are in the presentation.

use deckcraft_screen::{CaptureOptions, CaptureRegion};
use serde_json::{Value, json};

use super::{CommandSpec, bad, bool_or, cmd, has_slide, str_param, usize_param};
use crate::screen::ScreenState;
use crate::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "show.screenRecord",
            "Screen Recording",
            ["Record", "Record"],
            None,
            "{action?: start|pause|resume|stop|cancel|targets|permission, target?: id, region?: [x,y,w,h] px, fps?: 1-60, cursor?: bool, quality?: 10-51} → {state, …}",
            has_slide,
            run
        ),
    ]
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

fn target(p: &Value) -> Option<u64> {
    p.get("target").and_then(Value::as_u64).filter(|v| *v > 0)
}

fn state_json(s: &ScreenState) -> Value {
    json!({
        "state": if s.is_paused() { "paused" } else { "recording" },
        "frames": s.frames(),
        "durationMs": s.elapsed_ms(),
    })
}

fn run(s: &mut Session, p: &Value) -> crate::Result<Value> {
    let action = str_param(p, "action").unwrap_or("start").trim().to_ascii_lowercase();
    match action.as_str() {
        "start" => {
            let fps = fps(p)?;
            let region = region(p)?;
            let show_cursor = bool_or(p, "cursor", false);
            let options = CaptureOptions { target: target(p), region, fps, show_cursor };
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
            s.screen.pause().map_err(err)?;
            Ok(state_json(&s.screen))
        }
        "resume" => {
            s.screen.resume().map_err(err)?;
            Ok(state_json(&s.screen))
        }
        "cancel" => {
            let was = s.screen.cancel();
            Ok(json!({"state": if was { "cancelled" } else { "idle" }}))
        }
        "stop" => {
            let stopped = s.screen.stop().map_err(err)?;
            // Hide the temporary file name from the deck: the shape is named "Screen Recording".
            let insert = s.execute("insert.video", &json!({"path": stopped.path.to_string_lossy(), "name": "Screen Recording.mp4"}));
            let _ = std::fs::remove_file(&stopped.path);
            let inserted = insert?;
            Ok(json!({
                "state": "stopped",
                "id": inserted.get("id").cloned().unwrap_or(Value::Null),
                "frames": stopped.frames,
                "durationMs": stopped.duration_ms,
                "width": stopped.width,
                "height": stopped.height,
            }))
        }
        "targets" => {
            let targets = s.screen.targets().map_err(err)?;
            let list: Vec<Value> = targets
                .iter()
                .map(|t| {
                    json!({"id": t.id, "kind": t.kind.as_str(), "title": t.title, "width": t.width, "height": t.height})
                })
                .collect();
            Ok(json!({"targets": list, "status": status_str(s.screen.status())}))
        }
        "permission" => {
            let granted = s.screen.request_permission();
            Ok(json!({"granted": granted, "status": status_str(s.screen.status())}))
        }
        other => Err(bad(CMD, format!("action “{other}” isn't start, pause, resume, stop, cancel, targets or permission"))),
    }
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
    fn without_a_host_screen_recording_reports_unavailable() {
        let mut s = Session::with_new();
        let e = s.execute(CMD, &json!({"action": "start"})).unwrap_err().to_string();
        assert!(e.contains("isn't available"), "{e}");
    }

    #[test]
    fn stop_without_a_recording_is_a_plain_error_not_a_panic() {
        let mut s = Session::with_new();
        let e = s.execute(CMD, &json!({"action": "stop"})).unwrap_err().to_string();
        assert!(e.contains("no screen recording"), "{e}");
    }
}
