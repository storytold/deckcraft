//! Camera Format ▸ Camera ▸ Preview: the live camera toggle and the state it reports.
//!
//! The camera itself belongs to the UI host: [`crate::UiRequest::CameraPreview`] asks it to start
//! or stop the source, and it writes the outcome back to [`Session::camera_status`]. Nothing here
//! touches the presentation, so toggling the preview is not an undo step and never dirties a deck.

use serde_json::{Value, json};

use super::camera::has_cameo_or_preview;
use super::{CommandSpec, always, cmd};
use crate::{Result, Session, UiRequest};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            noundo "cameo.preview",
            "Enable Camera Preview",
            ["Camera Format", "Camera"],
            None,
            "{on?: bool, device?: string} → {on, device}",
            has_cameo_or_preview,
            preview
        ),
        cmd!(
            query "cameo.preview.state",
            "Camera Preview State",
            ["Camera Format", "Camera"],
            None,
            "{} → {state, device}",
            always,
            state
        ),
    ]
}

/// Ask the UI host to toggle, start or stop the preview. `on` absent means "flip it"; `device`
/// names the camera to open and is ignored when `on` is `false`.
fn preview(s: &mut Session, p: &Value) -> Result<Value> {
    let on = p.get("on").and_then(Value::as_bool);
    let device = p.get("device").and_then(Value::as_str).map(str::to_string);
    s.ui_requests.push(UiRequest::CameraPreview { on, device: device.clone() });
    Ok(json!({"on": on, "device": device}))
}

/// The state the UI host last reported (off before any preview, or with no host).
fn state(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(json!({"state": s.camera_status.as_str(), "device": s.camera_device}))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{Session, UiRequest};

    fn with_cameo() -> Session {
        let mut s = Session::with_new();
        s.execute("insert.cameo", &json!({})).expect("cameo");
        s
    }

    #[test]
    fn preview_needs_a_cameo_selected() {
        let mut s = Session::with_new();
        s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 40, 40]})).expect("shape");
        assert!(s.execute("cameo.preview", &json!({})).is_err(), "a plain shape has no camera preview");
        assert!(s.ui_requests.is_empty(), "the disabled command asks the UI for nothing");

        let mut s = with_cameo();
        s.execute("cameo.preview", &json!({})).expect("cameo preview");
        assert!(matches!(s.ui_requests.first(), Some(UiRequest::CameraPreview { on: None, device: None })));
    }

    #[test]
    fn preview_does_not_change_the_document_or_add_an_undo_step() {
        let mut s = with_cameo();
        let before = {
            let doc = s.active().expect("doc");
            (doc.revision, doc.history.undo.len(), doc.is_dirty())
        };
        s.execute("cameo.preview", &json!({})).expect("toggle");
        let after = {
            let doc = s.active().expect("doc");
            (doc.revision, doc.history.undo.len(), doc.is_dirty())
        };
        assert_eq!(before, after, "the preview is editor state, not a document edit");
    }

    #[test]
    fn preview_carries_an_explicit_state_and_device() {
        let mut s = with_cameo();
        s.execute("cameo.preview", &json!({"on": true, "device": "cam-2"})).expect("start");
        assert_eq!(s.ui_requests.pop(), Some(UiRequest::CameraPreview { on: Some(true), device: Some("cam-2".into()) }));
        s.execute("cameo.preview", &json!({"on": false})).expect("stop");
        assert_eq!(s.ui_requests.pop(), Some(UiRequest::CameraPreview { on: Some(false), device: None }));
    }

    #[test]
    fn state_reports_what_the_host_last_saw() {
        let mut s = Session::with_new();
        assert_eq!(s.execute("cameo.preview.state", &json!({})).expect("state"), json!({"state": "off", "device": null}));
        s.camera_status = deckcraft_media::camera::CameraStatus::Live;
        s.camera_device = Some("Integrated Camera".into());
        assert_eq!(s.execute("cameo.preview.state", &json!({})).expect("state"), json!({"state": "live", "device": "Integrated Camera"}));
    }
}
