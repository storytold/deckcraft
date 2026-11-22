//! Audio Format and Video Format: a selected clip's picture controls are on its own Format tab,
//! apart from Playback.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

/// Half a second of silence as a WAV file.
fn wav() -> Vec<u8> {
    let data = vec![0u8; 8000];
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

/// The clip's Format tab comes before Playback in the tab row, as in the layout these tabs follow.
fn format_tab_before_playback(h: &Harness<'_, SlideApp>, format: &str) {
    let (f, p) = (h.get_by_label(format).rect(), h.get_by_label("Playback").rect());
    assert!(f.max.x <= p.min.x, "{format} at {f:?} is not before Playback at {p:?}");
}

/// A blank slide with one selected clip inserted by `cmd` from `name`/`bytes`, showing `tab`.
fn with_clip(cmd: &str, name: &str, bytes: &[u8], tab: &str) -> Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    s.execute("slide.layout", &json!({"layout": "blank"})).unwrap();
    let id = s.execute(cmd, &json!({"name": name, "data": deckcraft_engine::cmd::base64_encode(bytes)})).unwrap()["id"].as_u64().unwrap();
    s.execute("edit.select", &json!({"ids": [id]})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.tab = tab.into();
    let mut h = Harness::builder().with_size(egui::vec2(1920.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

#[test]
fn an_audio_clip_has_an_audio_format_tab_with_its_picture() {
    let h = with_clip("insert.audio", "tone.wav", &wav(), "Audio Format");
    assert!(h.query_by_label("Video Format").is_none(), "no Video Format tab for audio");
    format_tab_before_playback(&h, "Audio Format");
    for label in ["Change Picture", "Reset Picture"] {
        assert!(h.query_by_label(label).is_some(), "{label} on Audio Format");
    }
}

#[test]
fn a_video_has_a_video_format_tab_with_its_poster_frame() {
    let clip = include_bytes!("../../../media/tests/fixtures/clip-h264.mp4");
    let h = with_clip("insert.video", "clip.mp4", clip, "Video Format");
    assert!(h.query_by_label("Audio Format").is_none(), "no Audio Format tab for video");
    format_tab_before_playback(&h, "Video Format");
    for label in ["Play", "Poster Frame", "Reset Design"] {
        assert!(h.query_by_label(label).is_some(), "{label} on Video Format");
    }
}
