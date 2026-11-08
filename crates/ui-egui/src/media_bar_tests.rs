//! The play/pause and timeline bar under a selected audio or video in the editor, clicked with the
//! mouse: a press on it used to reach the canvas, start a selection marquee there and hide the bar
//! before the click landed, so Play only worked in the slide show.

use deckcraft_engine::Session;
use deckcraft_model::ShapeId;
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

/// #1922: the bar hangs 6 pt under the clip, so a clip at the bottom edge of the slide put it
/// inside the speaker-notes pane, painted over the notes text. The canvas is clipped to its own
/// rect: nothing it paints reaches the notes pane, and a click there goes to the notes field.
#[test]
fn the_media_bar_never_paints_or_clicks_into_the_notes_pane() {
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let r = s.execute("insert.audio", &json!({"name": "tone.wav", "data": deckcraft_engine::cmd::base64_encode(&wav(2.0))})).unwrap();
    let id = ShapeId(r["id"].as_u64().unwrap() as u32);
    // Dragged across the bottom edge of the slide, onto the pasteboard.
    let h_slide = s.doc().unwrap().doc.slide_size.height;
    s.execute("shape.setBounds", &json!({"ids": [id.0], "x": 300.0, "y": h_slide - 10.0, "w": 120.0, "h": 60.0})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.notes = true;
    app.ui.notes_height = 140.0;
    let mut h = Harness::builder().with_size(vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    let notes = h.query_all_by_role(Role::MultilineTextInput).next().map(|n| n.rect()).expect("the notes field");
    let canvas = h.state().canvas_rect.unwrap();
    assert!(canvas.max.y <= notes.min.y + 1.0, "the notes pane sits under the canvas: {canvas:?} / {notes:?}");
    let d = h.state().session.active().unwrap();
    let x = d.doc.slides[d.selection.slide].shape(id).unwrap().xfrm.unwrap();
    let (slide, k) = (h.state().slide_rect.unwrap(), h.state().canvas_scale);
    let bottom = slide.min.y + (x.y + x.h) as f32 * k;
    let bar_y = bottom + 6.0 + 13.0;
    assert!(bar_y > notes.min.y, "the bar's row ({bar_y}) would fall inside the notes pane (top {})", notes.min.y);

    // The bar's own background (the only dark rounded rect the canvas paints there) has no
    // visible area below the canvas edge: whatever the canvas paints is clipped to the canvas.
    let bar_fill = egui::Color32::from_rgba_unmultiplied(28, 28, 32, 230);
    let bars: Vec<egui::Rect> = h
        .output()
        .shapes
        .iter()
        .filter_map(|cs| match &cs.shape {
            egui::Shape::Rect(r) if r.fill == bar_fill => Some(r.rect.intersect(cs.clip_rect)),
            _ => None,
        })
        .collect();
    assert!(!bars.is_empty(), "the bar is still laid out for the selected clip");
    assert!(bars.iter().all(|v| !v.is_positive() || v.max.y <= canvas.max.y + 0.5), "the bar is painted into the notes pane: {bars:?}");

    // A click where the bar would be lands in the notes field, not on the bar.
    click(&mut h, pos2(slide.min.x + (x.x + x.w / 2.0) as f32 * k, bar_y));
    assert!(h.state().media.status(id).is_none(), "Play was not hit through the notes pane");
    assert!(h.ctx.memory(|m| m.focused()).is_some(), "the notes field took the click");
}
