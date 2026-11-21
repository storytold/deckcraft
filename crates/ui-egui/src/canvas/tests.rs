#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use deckcraft_engine::Session;
use egui::{Event, Modifiers, MouseWheelUnit, Pos2, TouchPhase, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use super::*;
use crate::{Services, SlideApp};

/// #745: the resize cursor follows the handle's on-screen diagonal. A 45°-rotated shape's
/// NW corner sits at the top-right of the screen, where the drag axis is NE-SW — the old
/// fixed handle-to-cursor mapping showed NW-SE there.
#[test]
fn resize_cursor_tracks_rotation() {
    let mut s = Session::with_new();
    // A square shape: the NW handle's diagonal is at 45° unrotated, so each 45° of shape
    // rotation cleanly moves the cursor to the next of the four resize directions.
    let id = s.execute("shape.insert", &serde_json::json!({"preset": "rect", "rect": [100.0, 100.0, 100.0, 100.0]})).unwrap()["id"].as_u64().unwrap()
        as u32;
    let sid = deckcraft_model::ShapeId(id);
    // Unrotated: the NW handle (0) drags the NW-SE diagonal.
    assert_eq!(resize_cursor(&s, sid, 0), egui::CursorIcon::ResizeNwSe);
    assert_eq!(resize_cursor(&s, sid, 4), egui::CursorIcon::ResizeNwSe);
    assert_eq!(resize_cursor(&s, sid, 2), egui::CursorIcon::ResizeNeSw);
    assert_eq!(resize_cursor(&s, sid, 6), egui::CursorIcon::ResizeNeSw);
    assert_eq!(resize_cursor(&s, sid, 1), egui::CursorIcon::ResizeVertical);
    assert_eq!(resize_cursor(&s, sid, 3), egui::CursorIcon::ResizeHorizontal);
    // 45° rotation: the NW corner's drag axis rotates 45° with the shape — it is now vertical.
    {
        let doc = s.doc_mut().unwrap();
        let pres = Arc::make_mut(&mut doc.doc);
        let slide = Arc::make_mut(&mut pres.slides[0]);
        if let Some(sh) = slide.shapes.iter_mut().find(|x| x.id == sid)
            && let Some(x) = sh.xfrm.as_mut()
        {
            x.rot = 45.0;
        }
    }
    assert_eq!(resize_cursor(&s, sid, 0), egui::CursorIcon::ResizeVertical, "45°: NW handle drags vertically");
    assert_eq!(resize_cursor(&s, sid, 2), egui::CursorIcon::ResizeHorizontal, "45°: NE handle drags horizontally");
    // 90° rotation: the NW corner is at the top-right; the drag axis is NE-SW.
    {
        let doc = s.doc_mut().unwrap();
        let pres = Arc::make_mut(&mut doc.doc);
        let slide = Arc::make_mut(&mut pres.slides[0]);
        if let Some(sh) = slide.shapes.iter_mut().find(|x| x.id == sid)
            && let Some(x) = sh.xfrm.as_mut()
        {
            x.rot = 90.0;
        }
    }
    assert_eq!(resize_cursor(&s, sid, 0), egui::CursorIcon::ResizeNeSw, "90°: the diagonal is NE-SW");
    assert_eq!(resize_cursor(&s, sid, 2), egui::CursorIcon::ResizeNwSe, "90°: NE diagonal is NW-SE");
    assert_eq!(resize_cursor(&s, sid, 1), egui::CursorIcon::ResizeHorizontal, "90°: the top edge is horizontal");
}

/// #745: a corner handle shows a diagonal cursor whatever the shape's proportions. The
/// corner's direction is the diagonal, not the line to the box's centre, so a wide
/// rectangle's NW corner still drags along the NW-SE diagonal.
#[test]
fn a_wide_shape_keeps_its_corner_cursors_diagonal() {
    let mut s = Session::with_new();
    let id = s.execute("shape.insert", &serde_json::json!({"preset": "rect", "rect": [100.0, 100.0, 400.0, 80.0]})).unwrap()["id"].as_u64().unwrap()
        as u32;
    let sid = deckcraft_model::ShapeId(id);
    assert_eq!(resize_cursor(&s, sid, 0), egui::CursorIcon::ResizeNwSe, "the NW corner drags NW-SE");
    assert_eq!(resize_cursor(&s, sid, 4), egui::CursorIcon::ResizeNwSe, "the SE corner drags NW-SE");
    assert_eq!(resize_cursor(&s, sid, 2), egui::CursorIcon::ResizeNeSw, "the NE corner drags NE-SW");
    assert_eq!(resize_cursor(&s, sid, 6), egui::CursorIcon::ResizeNeSw, "the SW corner drags NE-SW");
    // The edges keep their own cursors.
    assert_eq!(resize_cursor(&s, sid, 1), egui::CursorIcon::ResizeVertical, "the top edge drags vertically");
    assert_eq!(resize_cursor(&s, sid, 3), egui::CursorIcon::ResizeHorizontal, "the right edge drags horizontally");
}

/// #745: a placeholder that inherits its box from the layout still shows a resize cursor.
/// Its handles are drawn from the inherited box, so the cursor follows that one.
#[test]
fn a_placeholder_that_inherits_its_box_has_a_resize_cursor() {
    let s = Session::with_new();
    let st = s.doc().unwrap();
    let ph = st
        .current_slide()
        .unwrap()
        .shapes
        .iter()
        .find(|sh| sh.xfrm.is_none() && sh.ph.is_some())
        .expect("the new slide has a placeholder without a box of its own")
        .id;
    assert_eq!(resize_cursor(&s, ph, 0), egui::CursorIcon::ResizeNwSe, "the NW corner drags NW-SE");
    assert_eq!(resize_cursor(&s, ph, 2), egui::CursorIcon::ResizeNeSw, "the NE corner drags NE-SW");
    assert_eq!(resize_cursor(&s, ph, 1), egui::CursorIcon::ResizeVertical, "the top edge drags vertically");
}

fn harness() -> Harness<'static, SlideApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    h.run_steps(3);
    h
}

/// The slide point (in slide points) under screen position `p`.
fn slide_point(h: &Harness<'_, SlideApp>, p: Pos2) -> Pos2 {
    let (r, s) = (h.state().slide_rect.unwrap(), h.state().canvas_scale);
    pos2((p.x - r.min.x) / s, (p.y - r.min.y) / s)
}

/// Test helper: inject a platform-command-modified line wheel event and run thirty harness
/// steps.
fn ctrl_wheel(h: &mut Harness<'_, SlideApp>, lines: f32) {
    let wheel = Event::MouseWheel {
        unit: MouseWheelUnit::Line,
        delta: vec2(0.0, lines),
        phase: TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: Modifiers::COMMAND,
    };
    h.event(wheel);
    // egui spreads a wheel notch over several frames.
    h.run_steps(30);
}

/// Within half a slide point, plus the half device pixel the slide moves to sit on the pixel
/// grid (#184).
fn assert_near(a: Pos2, b: Pos2, what: &str) {
    assert!((a - b).length() < 1.25, "{what}: {a:?} moved to {b:?}");
}

/// #106: wheel and pinch zoom zoomed toward the slide's corner instead of the pointer.
#[test]
fn wheel_and_pinch_zoom_keep_the_point_under_the_pointer() {
    let mut h = harness();
    let fit = h.state().canvas_scale;
    let r = h.state().slide_rect.unwrap();
    let p = r.min + vec2(r.width() * 0.8, r.height() * 0.7);
    h.event(Event::PointerMoved(p));
    h.run_steps(1);
    let under = slide_point(&h, p);

    h.event(Event::Zoom(2.0));
    h.run_steps(2);
    assert!((h.state().canvas_scale - fit * 2.0).abs() < 1e-3);
    assert_near(under, slide_point(&h, p), "pinch in");

    ctrl_wheel(&mut h, 1.0);
    assert!(h.state().canvas_scale > fit * 2.1, "Ctrl/Cmd + wheel up zooms in");
    assert_near(under, slide_point(&h, p), "wheel in");

    ctrl_wheel(&mut h, -1.0);
    ctrl_wheel(&mut h, -1.0);
    assert!(h.state().canvas_scale < fit * 2.0, "Ctrl/Cmd + wheel down zooms out");
    assert_near(under, slide_point(&h, p), "wheel out");

    h.event(Event::Zoom(0.8));
    h.run_steps(2);
    assert_near(under, slide_point(&h, p), "pinch out");

    // Zoomed out below the window size, the slide is centred again.
    h.event(Event::Zoom(0.25));
    h.run_steps(2);
    assert!(h.state().canvas_scale < fit);
    let (canvas, slide) = (h.state().canvas_rect.unwrap(), h.state().slide_rect.unwrap());
    assert!((canvas.center() - slide.center()).length() < 1.0, "{canvas:?} {slide:?}");
}

/// Zoom In/Out (keys, buttons, slider) keeps the middle of the view where it is.
#[test]
fn zoom_commands_keep_the_middle_of_the_view() {
    let mut h = harness();
    let c = h.state().canvas_rect.unwrap().center();
    let mid = slide_point(&h, c);
    for _ in 0..4 {
        h.state_mut().run("view.zoomIn", serde_json::json!({})).unwrap();
        h.run_steps(2);
        assert_near(mid, slide_point(&h, c), "zoom in");
    }
    h.state_mut().run("view.zoomOut", serde_json::json!({})).unwrap();
    h.run_steps(2);
    assert_near(mid, slide_point(&h, c), "zoom out");
}

/// Drags with the primary button from `a` to `b`; `during` runs after every move.
fn drag_with(h: &mut Harness<'_, SlideApp>, a: Pos2, b: Pos2, mut during: impl FnMut(&SlideApp)) {
    h.hover_at(a);
    h.run_steps(1);
    h.event(Event::PointerButton { pos: a, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    for i in 1..=8 {
        h.event(Event::PointerMoved(a + (b - a) * (i as f32 / 8.0)));
        h.run_steps(1);
        during(h.state());
    }
    h.event(Event::PointerButton { pos: b, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

/// #1157: a slide zoomed past the window has a scroll bar on each axis that overflows, and
/// dragging its thumb pans the slide, not a marquee on the canvas.
#[test]
fn scroll_bars_pan_a_zoomed_slide() {
    let mut h = harness();
    let fit = h.state().canvas_scale;
    h.state_mut().ui.zoom = Some(fit * 3.0);
    h.run_steps(3);
    let canvas = h.state().canvas_rect.unwrap();
    let before = h.state().slide_rect.unwrap();
    let no_marquee = |app: &SlideApp| assert!(app.session.tool.marquee.is_none() && !app.session.tool.dragging(), "the bar drag reached the slide");

    // Zoomed about the middle, so each thumb sits in the middle of its track.
    let bottom = pos2(canvas.center().x, canvas.max.y - 4.0);
    drag_with(&mut h, bottom, bottom + vec2(150.0, 0.0), no_marquee);
    let after = h.state().slide_rect.unwrap();
    assert!(before.min.x - after.min.x > 150.0, "the horizontal thumb pans right: {before:?} -> {after:?}");
    assert!((before.min.y - after.min.y).abs() < 1.0, "only across: {before:?} -> {after:?}");

    let right = pos2(canvas.max.x - 4.0, canvas.center().y);
    drag_with(&mut h, right, right + vec2(0.0, 100.0), no_marquee);
    let down = h.state().slide_rect.unwrap();
    assert!(after.min.y - down.min.y > 100.0, "the vertical thumb pans down: {after:?} -> {down:?}");
    assert!((after.min.x - down.min.x).abs() < 1.0, "only down: {after:?} -> {down:?}");

    // A click on the track beside the thumb pages back toward it.
    let left = pos2(canvas.min.x + 20.0, canvas.max.y - 4.0);
    h.hover_at(left);
    h.run_steps(1);
    h.event(Event::PointerButton { pos: left, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.event(Event::PointerButton { pos: left, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    let paged = h.state().slide_rect.unwrap();
    assert!(paged.min.x - down.min.x > 100.0, "a track click pages left: {down:?} -> {paged:?}");
    assert!(h.state().session.tool.marquee.is_none());

    // The ends of the tracks reach the slide's far edges.
    drag_with(&mut h, right, right + vec2(0.0, 2000.0), |_| {});
    let end = h.state().slide_rect.unwrap();
    assert!((end.max.y - (canvas.max.y - 28.0)).abs() < 2.0, "the bottom of the slide comes into view: {end:?} in {canvas:?}");
}

/// A press on the thumb that ends just past its end is a nudge, not a page: the track click
/// is decided where the button went down, not where it came up.
#[test]
fn a_short_press_on_the_thumb_does_not_page() {
    let mut h = harness();
    let fit = h.state().canvas_scale;
    h.state_mut().ui.zoom = Some(fit * 3.0);
    h.run_steps(3);
    let (canvas, before) = (h.state().canvas_rect.unwrap(), h.state().slide_rect.unwrap());
    let overflow = vec2((before.width() + 56.0 - canvas.width()).max(0.0), (before.height() + 56.0 - canvas.height()).max(0.0));
    let offset = canvas.min + vec2(28.0, 28.0) - before.min;
    let (_, thumb, _) = scroll_bar(canvas, 0.0, overflow, offset, 0).unwrap();
    let (a, b) = (pos2(thumb.max.x - 3.0, canvas.max.y - 4.0), pos2(thumb.max.x + 3.0, canvas.max.y - 4.0));
    h.hover_at(a);
    h.run_steps(1);
    h.event(Event::PointerButton { pos: a, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.event(Event::PointerMoved(b));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: b, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    let after = h.state().slide_rect.unwrap();
    let moved = before.min.x - after.min.x;
    assert!((0.0..40.0).contains(&moved), "a 6 pt nudge of the thumb, not a page: {before:?} -> {after:?}");
}

/// Only the primary button drags a thumb: a right or middle drag after a left one leaves the
/// slide where it is.
#[test]
fn only_the_primary_button_drags_a_thumb() {
    let mut h = harness();
    let fit = h.state().canvas_scale;
    h.state_mut().ui.zoom = Some(fit * 3.0);
    h.run_steps(3);
    let canvas = h.state().canvas_rect.unwrap();
    let bottom = pos2(canvas.center().x, canvas.max.y - 4.0);
    drag_with(&mut h, bottom, bottom + vec2(40.0, 0.0), |_| {});
    let before = h.state().slide_rect.unwrap();
    for button in [egui::PointerButton::Secondary, egui::PointerButton::Middle] {
        let a = pos2(canvas.center().x + 40.0, canvas.max.y - 4.0);
        h.hover_at(a);
        h.run_steps(1);
        h.event(Event::PointerButton { pos: a, button, pressed: true, modifiers: Modifiers::NONE });
        h.run_steps(1);
        for i in 1..=4 {
            h.event(Event::PointerMoved(a + vec2(30.0 * i as f32, 0.0)));
            h.run_steps(1);
        }
        h.event(Event::PointerButton { pos: a + vec2(120.0, 0.0), button, pressed: false, modifiers: Modifiers::NONE });
        h.run_steps(2);
        let after = h.state().slide_rect.unwrap();
        assert!((before.min - after.min).length() < 1.0, "{button:?} drag moved the slide: {before:?} -> {after:?}");
    }
}

/// A slide that fits has no scroll bars: a drag along the canvas edge is a marquee.
#[test]
fn a_fitted_slide_has_no_scroll_bars() {
    let mut h = harness();
    let canvas = h.state().canvas_rect.unwrap();
    let a = pos2(canvas.center().x, canvas.max.y - 4.0);
    let mut marquee = false;
    drag_with(&mut h, a, a + vec2(150.0, 0.0), |app| marquee |= app.session.tool.marquee.is_some());
    assert!(marquee, "the drag selects on the canvas");
}

fn app_with_table() -> (egui_kittest::Harness<'static, SlideApp>, deckcraft_model::ShapeId) {
    let mut s = Session::with_new();
    let data = json!([["Hello world", "b"], ["c", ""]]);
    let r = s.execute("insert.table", &json!({"rows": 2, "cols": 2, "rect": [100.0, 100.0, 400.0, 80.0], "data": data})).unwrap();
    let id = deckcraft_model::ShapeId(r["id"].as_u64().unwrap() as u32);
    s.execute("edit.deselect", &json!({})).unwrap();
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(s, Services::default()),
    );
    // The app keeps requesting repaints, so step frames instead of run().
    harness.run_steps(2);
    (harness, id)
}

/// Screen position of slide point (x, y).
fn screen(app: &SlideApp, x: f64, y: f64) -> Pos2 {
    Xf { origin: app.slide_rect.unwrap().min, scale: app.canvas_scale }.to_screen(Point::new(x, y))
}

/// Press and release at `p`, 50 ms apart in input time (two clicks make a double-click).
fn click(harness: &mut egui_kittest::Harness<'static, SlideApp>, p: Pos2) {
    for pressed in [true, false] {
        let t = harness.ctx.input(|i| i.time) + 0.05;
        harness.input_mut().time = Some(t);
        harness.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        harness.step();
    }
}

/// The first empty placeholder and the paragraph after Enter keep a list marker and indent.
#[test]
fn empty_bullet_preview_follows_click_type_enter_and_toggle() {
    let mut h = harness();
    h.state_mut().run("slide.new", json!({"layout": "titleAndContent"})).unwrap();
    h.run_steps(3);
    let st = h.state().session.active().unwrap();
    let sh =
        st.shapes().iter().find(|s| s.ph_type() == Some(deckcraft_model::PhType::Body) || s.ph_type() == Some(deckcraft_model::PhType::Obj)).unwrap();
    let id = sh.id;
    let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh);
    let p = screen(h.state(), x.x + 40.0, x.y + 20.0);
    click(&mut h, p);
    h.run_steps(3);
    let caret = |h: &Harness<'_, SlideApp>| {
        let st = h.state().session.active().unwrap();
        let ts = st.selection.text.as_ref().expect("click starts typing");
        assert_eq!(ts.shape, id);
        deckcraft_engine::cmd::text::caret_line(st, ts).unwrap().0
    };
    let first = caret(&h);
    h.event(Event::Text("First point".into()));
    h.run_steps(3);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    let second = caret(&h);
    assert!((first.x - second.x).abs() < 0.01, "empty lines retain the bullet indent");
    assert!(second.y > first.y);
    let st = h.state().session.active().unwrap();
    let l = deckcraft_engine::cmd::text::layout_for(st, st.selection.text.as_ref().unwrap()).unwrap();
    assert_eq!(l.empty_bullets.len(), 1);
    assert_eq!(l.empty_bullets[0].para, 1);
    let renders = h.state().textures.canvas_renders;
    h.state_mut().run("text.exit", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.state().textures.canvas_renders > renders, "leaving text editing refreshes the marker texture");
    h.state_mut().run("text.edit", json!({"id": id, "at": [1, 0]})).unwrap();
    h.run_steps(3);
    h.event(Event::Text("Second point".into()));
    h.run_steps(3);
    let st = h.state().session.active().unwrap();
    let ts = st.selection.text.as_ref().unwrap();
    assert_eq!(deckcraft_engine::cmd::text::body_of(st, ts).unwrap().text(), "First point\nSecond point");
    assert!(deckcraft_engine::cmd::text::layout_for(st, ts).unwrap().empty_bullets.is_empty());
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    h.state_mut().run("format.bullets", json!({})).unwrap();
    h.run_steps(3);
    let st = h.state().session.active().unwrap();
    assert!(deckcraft_engine::cmd::text::layout_for(st, st.selection.text.as_ref().unwrap()).unwrap().empty_bullets.is_empty());
}

/// Double-clicking a table cell edits it with the caret drawn in that cell, where clicked.
#[test]
fn double_click_in_a_table_cell_shows_the_caret_there() {
    let (mut harness, id) = app_with_table();
    // The empty cell (1, 1) spans x 300..500, y 140..180.
    let p = screen(harness.state(), 400.0, 160.0);
    harness.hover_at(p);
    harness.step();
    click(&mut harness, p);
    click(&mut harness, p);
    harness.run_steps(2);
    let st = harness.state().session.active().unwrap();
    let ts = st.selection.text.clone().expect("editing the cell");
    assert_eq!((ts.shape, ts.cell), (id, Some((1, 1))));
    assert!(text_marks(st, &ts).is_some_and(|m| m.selection.is_empty()), "no range");
    let (top, bottom) = deckcraft_engine::cmd::text::caret_line(st, &ts).expect("the cell's text has a caret");
    for q in [top, bottom] {
        assert!(q.x > 300.0 && q.x < 500.0 && q.y >= 140.0 && q.y <= 180.0, "caret {q:?} outside cell (1, 1)");
    }
    // A click in another cell moves the caret there.
    let p = screen(harness.state(), 110.0, 120.0);
    click(&mut harness, p);
    harness.run_steps(1);
    let st = harness.state().session.active().unwrap();
    let ts = st.selection.text.clone().unwrap();
    assert_eq!((ts.cell, ts.caret), (Some((0, 0)), (0, 0)));
    let (top, _) = deckcraft_engine::cmd::text::caret_line(st, &ts).unwrap();
    assert!(top.x > 100.0 && top.x < 120.0 && top.y >= 100.0 && top.y < 140.0, "caret {top:?} at the start of cell (0, 0)");
}

/// Table borders show a resize cursor and drag to resize columns.
#[test]
fn table_borders_resize_with_the_pointer() {
    let (mut harness, id) = app_with_table();
    let p = screen(harness.state(), 300.0, 120.0);
    harness.hover_at(p);
    harness.step();
    harness.step();
    assert_eq!(harness.output().platform_output.cursor_icon, egui::CursorIcon::ResizeColumn);
    let q = screen(harness.state(), 340.0, 120.0);
    harness.drag_at(p);
    harness.step();
    for k in 1..=4 {
        harness.hover_at(p + (q - p) * k as f32 / 4.0);
        harness.step();
    }
    harness.drop_at(q);
    harness.run_steps(2);
    let st = harness.state().session.active().unwrap();
    let deckcraft_model::ShapeKind::Table(t) = &st.shape(id).unwrap().kind else { panic!("not a table") };
    assert!((t.cols[0] - 240.0).abs() < 0.5 && (t.cols[1] - 160.0).abs() < 0.5, "{:?}", t.cols);
}

fn right_click(h: &mut Harness<'_, SlideApp>, pos: Pos2) {
    h.event(Event::PointerMoved(pos));
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
        h.step();
    }
    h.run_steps(2);
}

/// In Edit Points, right-clicking a segment offers Straight/Curved Segment and right-clicking
/// a point offers Open Path, then Close Path once the outline is open.
#[test]
fn edit_points_menu_sets_segment_types_and_opens_the_path() {
    let mut h = harness();
    h.state_mut().run("slide.layout", json!({"layout": "blank"})).unwrap();
    h.state_mut().run("shape.insert", json!({"preset": "rect", "rect": [200, 150, 300, 200]})).unwrap();
    h.state_mut().run("shape.editPoints", json!({"on": true})).unwrap();
    h.run_steps(3);
    let q = |h: &mut Harness<'_, SlideApp>| h.state_mut().session.execute("shape.points", &json!({})).unwrap();
    // The middle of the right edge: the segment ending at point 2.
    let p = screen(h.state(), 500.0, 250.0);
    right_click(&mut h, p);
    h.get_by_label("Curved Segment").click();
    h.run_steps(3);
    assert_eq!(q(&mut h)["segments"][0], json!(["straight", "straight", "curved", "straight"]));
    // The bottom-right corner: Open Path breaks the outline there.
    let p = screen(h.state(), 500.0, 350.0);
    right_click(&mut h, p);
    h.get_by_label("Open Path").click();
    h.run_steps(3);
    assert_eq!(q(&mut h)["paths"][0].as_array().map(Vec::len), Some(5));
    right_click(&mut h, p);
    h.get_by_label("Close Path").click();
    h.run_steps(3);
    assert_eq!(q(&mut h)["paths"][0].as_array().map(Vec::len), Some(4));
}

/// In Edit Points, right-clicking a segment offers Add Point at the pointer, Delete Segment on
/// a closed outline, and Open Path at the segment's start point.
#[test]
fn edit_points_segment_menu_adds_points_and_deletes_segments() {
    let mut h = harness();
    h.state_mut().run("slide.layout", json!({"layout": "blank"})).unwrap();
    h.state_mut().run("shape.insert", json!({"preset": "rect", "rect": [200, 150, 300, 200]})).unwrap();
    h.state_mut().run("shape.editPoints", json!({"on": true})).unwrap();
    h.run_steps(3);
    let q = |h: &mut Harness<'_, SlideApp>| h.state_mut().session.execute("shape.points", &json!({})).unwrap();
    // A quarter of the way along the top edge: the new point goes there.
    let p = screen(h.state(), 275.0, 150.0);
    right_click(&mut h, p);
    h.get_by_label("Add Point").click();
    h.run_steps(3);
    assert_eq!(q(&mut h)["paths"][0], json!([[200.0, 150.0], [275.0, 150.0], [500.0, 150.0], [500.0, 350.0], [200.0, 350.0]]));
    // The right edge: Open Path opens the outline at its top end.
    let p = screen(h.state(), 500.0, 250.0);
    right_click(&mut h, p);
    h.get_by_label("Open Path").click();
    h.run_steps(3);
    assert_eq!(q(&mut h)["paths"][0][0], json!([500.0, 150.0]));
    assert_eq!(q(&mut h)["paths"][0].as_array().map(Vec::len), Some(6));
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(3);
    // Delete Segment removes the right edge: the outline now runs from its bottom end to its top end.
    right_click(&mut h, p);
    h.get_by_label("Delete Segment").click();
    h.run_steps(3);
    assert_eq!(q(&mut h)["paths"][0], json!([[500.0, 350.0], [200.0, 350.0], [200.0, 150.0], [275.0, 150.0], [500.0, 150.0]]));
    assert_eq!(q(&mut h)["segments"][0][0], json!(null));
    // On the open outline Delete Segment is grayed out; Close Path joins the ends again.
    let p = screen(h.state(), 350.0, 350.0);
    right_click(&mut h, p);
    assert!(h.get_by_label("Delete Segment").accesskit_node().is_disabled());
    h.get_by_label("Close Path").click();
    h.run_steps(3);
    assert_eq!(q(&mut h)["segments"][0][0], json!("straight"));
}

#[test]
fn grid_step_follows_spacing_and_rejects_unusable_values() {
    let step = super::grid_line_step(72.0, 0.5).unwrap_or(0.0);
    assert!((step - 36.0).abs() < 1e-3, "{step}");
    assert!(super::grid_line_step(6.0, 1.0).is_some());
    assert!(super::grid_line_step(0.0, 1.0).is_none());
    assert!(super::grid_line_step(f64::NAN, 1.0).is_none());
    assert!(super::grid_line_step(1_000.0, 1.0).is_none());
    assert!(super::grid_line_step(6.0, 0.0).is_none());
    assert!(super::grid_line_step(6.0, 0.1).is_none(), "a 6 pt grid at 10% zoom is finer than one screen point");
}

/// #830: when the Format or Animation Painter is armed, the cursor shows the copy icon so the
/// user can see the tool is waiting for a click.
#[test]
fn format_painter_sets_the_cursor() {
    let mut h = harness();
    // Hover over the canvas so the pointer function runs and sets the cursor.
    h.hover_at(egui::pos2(600.0, 400.0));
    h.run_steps(2);
    let before = h.output().platform_output.cursor_icon;
    assert_ne!(before, egui::CursorIcon::Copy, "no painter is armed yet");
    // Arming the format painter overrides the cursor to Copy (#830).
    h.state_mut().session.painter = Some((Default::default(), false));
    h.run_steps(3);
    let after = h.output().platform_output.cursor_icon;
    assert_eq!(after, egui::CursorIcon::Copy, "the format painter arms the Copy cursor");
}

/// #830: the Animation Painter arms the same cursor.
#[test]
fn animation_painter_sets_the_cursor() {
    let mut h = harness();
    h.hover_at(egui::pos2(600.0, 400.0));
    h.run_steps(2);
    let before = h.output().platform_output.cursor_icon;
    assert_ne!(before, egui::CursorIcon::Copy, "no painter is armed yet");
    h.state_mut().session.anim_painter = Some((vec![], false));
    h.run_steps(3);
    let after = h.output().platform_output.cursor_icon;
    assert_eq!(after, egui::CursorIcon::Copy, "the animation painter arms the Copy cursor");
}
