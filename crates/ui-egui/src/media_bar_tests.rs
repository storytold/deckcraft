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

/// A blank slide holding a 2 s audio clip (selected, as inserted) and a rectangle beside it.
fn clip_and_rect() -> (Harness<'static, SlideApp>, ShapeId, ShapeId) {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let rect = s.execute("shape.insert", &json!({"preset": "rect", "rect": [600.0, 60.0, 120.0, 80.0]})).unwrap()["id"].as_u64().unwrap();
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
    (h, id, ShapeId(rect as u32))
}

/// Where the clip's bar's Play button is on screen.
fn play_button(h: &Harness<'_, SlideApp>, id: ShapeId) -> Pos2 {
    let d = h.state().session.active().unwrap();
    let x = d.doc.slides[d.selection.slide].shape(id).unwrap().xfrm.unwrap();
    let (slide, k) = (h.state().slide_rect.unwrap(), h.state().canvas_scale);
    let (left, right, bottom) = (slide.min.x + x.x as f32 * k, slide.min.x + (x.x + x.w) as f32 * k, slide.min.y + (x.y + x.h) as f32 * k);
    let w = (right - left).max(230.0);
    pos2((left + right) / 2.0 - w / 2.0, bottom + 6.0) + vec2(15.0, 13.0)
}

fn select(h: &mut Harness<'_, SlideApp>, ids: &[ShapeId]) {
    let ids: Vec<u32> = ids.iter().map(|i| i.0).collect();
    h.state_mut().run("edit.select", json!({"ids": ids})).unwrap();
    h.run_steps(2);
}

fn bar_shown(h: &Harness<'_, SlideApp>) -> bool {
    use egui_kittest::kittest::Queryable;
    h.query_by_label("Play or pause media").is_some()
}

/// #2328: as in PowerPoint, clicking off a playing clip stops it and its bar goes; a clip kept
/// in a larger selection plays on; selecting it again starts it over.
#[test]
fn deselecting_a_playing_clip_stops_it_and_hides_its_bar() {
    let (mut h, id, rect) = clip_and_rect();
    let play = play_button(&h, id);
    click(&mut h, play);
    assert!(h.state().media.is_playing(id), "Play starts the clip");
    select(&mut h, &[id, rect]);
    assert!(h.state().media.is_playing(id), "still selected along with another shape: it plays on");
    // Away from the clip, so the bar would show only because the clip has a voice.
    h.hover_at(pos2(5.0, 895.0));
    select(&mut h, &[rect]);
    assert!(h.state().media.status(id).is_none(), "deselected: stopped");
    assert!(!bar_shown(&h), "and its bar is gone");
    select(&mut h, &[id]);
    assert!(h.state().media.status(id).is_none(), "selecting it again does not resume a paused position");
}

/// #2328: a clip played without selecting it (media.play from the CLI or MCP) plays on until the
/// selection changes.
#[test]
fn a_clip_played_without_selecting_it_plays_until_the_selection_changes() {
    let (mut h, id, rect) = clip_and_rect();
    select(&mut h, &[]);
    h.state_mut().run("media.play", json!({"id": id.0})).unwrap();
    h.run_steps(3);
    assert!(h.state().media.is_playing(id), "plays with nothing selected");
    select(&mut h, &[rect]);
    assert!(h.state().media.status(id).is_none(), "a selection change stops it");
}

/// #2328: the bar of a clip under the pointer plays it, and selects it, so clicking off it then
/// stops it as for a clip selected first.
#[test]
fn the_bar_of_an_unselected_clip_selects_it() {
    let (mut h, id, _) = clip_and_rect();
    // Wider than its bar, so the bar's Play button is under the clip: the hover bar shows only
    // while the pointer is within the clip's width.
    h.state_mut()
        .session
        .edit(|doc, sel| {
            let sh = deckcraft_model::find_shape_mut(&mut std::sync::Arc::make_mut(&mut doc.slides[sel.slide]).shapes, id).unwrap();
            sh.xfrm = Some(deckcraft_geom::Xfrm::new(100.0, 100.0, 400.0, 100.0));
            Ok(())
        })
        .unwrap();
    select(&mut h, &[]);
    let play = play_button(&h, id);
    // The bar shows for the clip under the pointer.
    let d = h.state().session.active().unwrap();
    let x = d.doc.slides[d.selection.slide].shape(id).unwrap().xfrm.unwrap();
    let (slide, k) = (h.state().slide_rect.unwrap(), h.state().canvas_scale);
    h.hover_at(pos2(slide.min.x + (x.x + x.w / 2.0) as f32 * k, slide.min.y + (x.y + x.h / 2.0) as f32 * k));
    h.run_steps(2);
    click(&mut h, play);
    assert!(h.state().media.is_playing(id), "the bar plays the clip");
    assert_eq!(h.state().session.active().unwrap().selection.shapes, vec![id], "and selects it");
}

/// #2329: leaving the slide stops the clip, also when the next slide has a clip with the same
/// shape id (shape ids of a .pptx repeat from slide to slide). Played with nothing selected, so
/// the slide change alone has to stop it: moving to another slide also clears the selection,
/// which stops a selected clip anyway (#2328).
#[test]
fn changing_slides_stops_a_clip_even_with_the_same_id_on_the_next_slide() {
    let (mut h, id, _) = clip_and_rect();
    select(&mut h, &[]);
    h.state_mut().run("media.play", json!({"id": id.0})).unwrap();
    h.run_steps(3);
    assert!(h.state().media.is_playing(id));
    h.state_mut().run("slide.new", json!({"layout": "blank"})).unwrap();
    h.state_mut()
        .session
        .edit(|doc, sel| {
            let same = doc.slides[sel.slide - 1].shapes.iter().find(|x| x.id == id).cloned().unwrap();
            std::sync::Arc::make_mut(&mut doc.slides[sel.slide]).shapes.push(same);
            Ok(())
        })
        .unwrap();
    h.run_steps(3);
    assert!(h.state().session.active().unwrap().selection.shapes.is_empty(), "the selection stayed empty");
    assert!(h.state().media.status(id).is_none(), "the first slide's clip stopped");
}
