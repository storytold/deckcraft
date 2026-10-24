//! The play/pause and timeline bar under a selected audio or video in the editor, clicked with the
//! mouse: a press on it used to reach the canvas, start a selection marquee there and hide the bar
//! before the click landed, so Play only worked in the slide show.

use std::sync::Arc;

use deckcraft_engine::Session;
use deckcraft_model::{Shape, ShapeId, ShapeKind};
use egui::{Modifiers, PointerButton, Pos2, pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

use crate::{Services, SlideApp};

/// 16-bit mono WAV of silence, `secs` long at 8 kHz.
fn wav(secs: f64) -> Vec<u8> {
    let data = vec![0u8; (8_000.0 * secs) as usize * 2];
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    for v in [16u32.to_le_bytes(), [1, 0, 1, 0], 8_000u32.to_le_bytes(), 16_000u32.to_le_bytes(), [2, 0, 16, 0]] {
        b.extend_from_slice(&v);
    }
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(data.len() as u32).to_le_bytes());
    b.extend_from_slice(&data);
    b
}

fn click(h: &mut Harness<'_, SlideApp>, p: Pos2) {
    h.hover_at(p);
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

#[test]
fn play_on_the_editor_media_bar_plays_the_selected_clip() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let r = s.execute("insert.audio", &json!({"name": "tone.wav", "data": deckcraft_engine::cmd::base64_encode(&wav(2.0))})).unwrap();
    let id = ShapeId(r["id"].as_u64().unwrap() as u32);
    let app = SlideApp::new(s, Services::default());
    let mut h = Harness::builder().with_size(vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    let d = h.state().session.active().unwrap();
    assert_eq!(d.selection.shapes, vec![id], "the inserted clip is selected");
    let x = d.doc.slides[d.selection.slide].shape(id).unwrap().xfrm.unwrap();
    let (slide, k) = (h.state().slide_rect.unwrap(), h.state().canvas_scale);
    // The bar sits 6 points under the clip, centred, at least 230 wide; Play is its first button.
    let (left, right, bottom) = (slide.min.x + x.x as f32 * k, slide.min.x + (x.x + x.w) as f32 * k, slide.min.y + (x.y + x.h) as f32 * k);
    let w = (right - left).max(230.0);
    let bar = pos2((left + right) / 2.0 - w / 2.0, bottom + 6.0);
    assert!(h.state().media.status(id).is_none());

    click(&mut h, bar + vec2(15.0, 13.0));
    assert!(h.state().media.status(id).is_some(), "Play starts the clip");
    let d = h.state().session.active().unwrap();
    assert_eq!(d.selection.shapes, vec![id], "the click did not reach the slide behind the bar");
    assert!(!h.state().session.tool.dragging());

    // Between the buttons and the timeline the bar still keeps the press from the slide.
    click(&mut h, bar + vec2(w - 30.0, 13.0));
    assert_eq!(h.state().session.active().unwrap().selection.shapes, vec![id]);
}

/// A click on a video plays it and the next one pauses it; dragging it only moves it.
#[test]
fn a_click_on_a_video_plays_or_pauses_it() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let r = s.execute("insert.audio", &json!({"name": "tone.wav", "data": deckcraft_engine::cmd::base64_encode(&wav(10.0))})).unwrap();
    let id = ShapeId(r["id"].as_u64().unwrap() as u32);
    // Only videos play on a click; the clip's sound stands in for a film. It is long enough to
    // still be playing when the last click lands.
    {
        let d = s.active_mut().unwrap();
        let slide = Arc::make_mut(&mut Arc::make_mut(&mut d.doc).slides[d.selection.slide]);
        if let Some(Shape { kind: ShapeKind::Media(clip), .. }) = deckcraft_model::find_shape_mut(&mut slide.shapes, id) {
            clip.video = true;
        }
    }
    s.execute("edit.deselect", &json!({})).unwrap();
    let app = SlideApp::new(s, Services::default());
    let mut h = Harness::builder().with_size(vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    let d = h.state().session.active().unwrap();
    let x = d.doc.slides[d.selection.slide].shape(id).unwrap().xfrm.unwrap();
    // Where the clip's centre is on screen (a double click opens the Format pane, which moves it).
    let centre_of = |h: &Harness<'_, SlideApp>| {
        let (slide, k) = (h.state().slide_rect.unwrap(), h.state().canvas_scale);
        slide.min + vec2((x.x + x.w / 2.0) as f32 * k, (x.y + x.h / 2.0) as f32 * k)
    };
    let mut centre = centre_of(&h);

    click(&mut h, centre);
    assert_eq!(h.state().session.active().unwrap().selection.shapes, vec![id], "the click selects it");
    assert!(h.state().media.is_playing(id), "and plays it");
    // Far enough apart not to make a double click.
    h.run_steps(40);
    click(&mut h, centre);
    assert!(!h.state().media.is_playing(id), "the next click pauses it");

    // A double or triple click plays it once: only the first click counts.
    for clicks in [2, 3] {
        h.run_steps(40);
        assert!(!h.state().media.is_playing(id));
        for _ in 0..clicks {
            h.event(egui::Event::PointerButton { pos: centre, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
            h.run_steps(1);
            h.event(egui::Event::PointerButton { pos: centre, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
            h.run_steps(1);
        }
        assert!(h.state().media.is_playing(id), "{clicks} quick clicks leave it playing");
        h.run_steps(40);
        centre = centre_of(&h);
        click(&mut h, centre);
        assert!(!h.state().media.is_playing(id));
    }

    // A click the Eyedropper takes does not play the video.
    h.run_steps(40);
    crate::eyedropper::note_open_menu(&h.ctx, egui::Id::new("fill-colour"));
    crate::eyedropper::request(&h.ctx);
    h.run_steps(2);
    assert!(h.state().eyedropper.is_some(), "the Eyedropper waits for a click");
    click(&mut h, centre);
    assert!(h.state().eyedropper.is_none(), "the Eyedropper took the click");
    assert!(!h.state().media.is_playing(id), "the video did not start");

    // A drag moves it without playing.
    h.run_steps(40);
    h.event(egui::Event::PointerButton { pos: centre, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    for i in 1..=6 {
        h.event(egui::Event::PointerMoved(centre + vec2(8.0 * i as f32, 0.0)));
        h.run_steps(1);
    }
    h.event(egui::Event::PointerButton { pos: centre + vec2(48.0, 0.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert!(!h.state().media.is_playing(id), "dragging does not play it");
    let d = h.state().session.active().unwrap();
    let moved = d.doc.slides[d.selection.slide].shape(id).unwrap().xfrm.unwrap();
    assert!(moved.x > x.x, "the drag moved it");
}
