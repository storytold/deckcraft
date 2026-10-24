//! The Animation Pane's advanced timeline driven with the mouse: dragging a bar changes the
//! delay, dragging its right edge changes the duration, each one undoable command.

use deckcraft_engine::Session;
use egui::Pos2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

/// A blank slide with two shapes: Fade on click (1 s), then Zoom after previous (2 s).
fn harness() -> Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    for (i, (effect, start, dur)) in [("fade", "onClick", 1000), ("zoom", "afterPrevious", 2000)].iter().enumerate() {
        let id =
            s.execute("shape.insert", &json!({"preset": "rect", "rect": [40.0 + 200.0 * i as f64, 80, 150, 100]})).unwrap()["id"].as_u64().unwrap();
        s.execute("edit.select", &json!({"ids": [id]})).unwrap();
        s.execute("animation.add", &json!({"effect": effect, "start": start, "duration": dur})).unwrap();
    }
    let mut app = SlideApp::new(s, Services::default());
    app.ui.pane = Some("animation".into());
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn timing(h: &Harness<'_, SlideApp>, i: usize) -> (u32, u32) {
    let a = &h.state().session.active().unwrap().current_slide().unwrap().animations[i];
    (a.delay_ms, a.duration_ms)
}

fn drag(h: &mut Harness<'_, SlideApp>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.run_steps(1);
    h.drag_at(from);
    h.run_steps(1);
    for i in 1..=6 {
        h.hover_at(from + (to - from) * (i as f32 / 6.0));
        h.run_steps(1);
    }
    h.drop_at(to);
    h.run_steps(3);
}

#[test]
fn after_previous_bar_starts_where_the_first_ends() {
    let h = harness();
    let a = h.get_by_label("Timing bar 1").rect();
    let b = h.get_by_label("Timing bar 2").rect();
    assert!((b.left() - a.right()).abs() < 1.5, "{a:?} {b:?}");
    assert!((b.width() - 2.0 * a.width()).abs() < 1.5, "2 s bar twice the 1 s bar");
}

#[test]
fn dragging_a_bar_sets_the_delay_and_undoes() {
    let mut h = harness();
    let a = h.get_by_label("Timing bar 1").rect();
    // One second is the width of bar 1.
    drag(&mut h, a.center(), a.center() + egui::vec2(a.width() / 2.0, 0.0));
    assert_eq!(timing(&h, 0), (500, 1000));
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(timing(&h, 0), (0, 1000));
}

#[test]
fn dragging_the_edge_sets_the_duration() {
    let mut h = harness();
    let b = h.get_by_label("Timing bar 2").rect();
    let one_sec = b.width() / 2.0;
    let edge = h.get_by_label("Duration handle 2").rect().center();
    drag(&mut h, edge, edge - egui::vec2(one_sec, 0.0));
    assert_eq!(timing(&h, 1), (0, 1000));
}

/// Seconds on the play-all clock where the canvas preview is, right after the pane's play button.
fn play(h: &mut Harness<'_, SlideApp>, label: &str) -> f64 {
    h.get_by_label(label).click();
    h.run_steps(2);
    let (_, at, _) = h.state().anim_preview_report().expect("preview running");
    h.state_mut().stop_anim_preview();
    h.run_steps(1);
    at
}

#[test]
fn play_from_starts_at_the_selected_effect() {
    let mut h = harness();
    // The harness leaves the zoomed shape selected: its effect starts once the 1 s fade is over.
    let at = play(&mut h, "▶ Play From");
    assert!((1.0..1.1).contains(&at), "{at}");
    let (shape, tl) = {
        let st = h.state().session.active().unwrap();
        let slide = st.current_slide().unwrap();
        (slide.animations[0].shape, deckcraft_anim::Timeline::new(slide, st.doc.slide_size.width, st.doc.slide_size.height))
    };
    assert!(tl.state_at(shape, None, at).visible, "the faded-in shape is already on the slide");

    h.state_mut().session.execute("edit.deselect", &json!({})).unwrap();
    h.run_steps(2);
    assert!(play(&mut h, "▶ Play All") < 0.1);
}

#[test]
fn play_from_follows_the_effect_clicked_in_the_pane() {
    let mut h = harness();
    // A second effect on the first shape, after the zoom: 3 s into the sequence.
    let first = h.state().session.active().unwrap().current_slide().unwrap().animations[0].shape.0;
    h.state_mut().session.execute("edit.select", &json!({"ids": [first]})).unwrap();
    h.state_mut().session.execute("animation.add", &json!({"effect": "pulse", "start": "afterPrevious", "duration": 1000})).unwrap();
    h.run_steps(2);
    // The row of the pulse, left of its bar.
    let bar = h.get_by_label("Timing bar 3").rect();
    let row = egui::pos2(bar.left() - 20.0, bar.center().y);
    h.hover_at(row);
    h.run_steps(1);
    h.drag_at(row);
    h.run_steps(1);
    h.drop_at(row);
    h.run_steps(2);
    let at = play(&mut h, "▶ Play From");
    assert!((3.0..3.1).contains(&at), "plays from the clicked row, not the shape's first effect: {at}");
}

/// Clicks the pane row of effect `n` (1-based), left of its timing bar, with `button`.
fn click_row(h: &mut Harness<'_, SlideApp>, n: usize, button: egui::PointerButton) {
    let bar = h.get_by_label(&format!("Timing bar {n}")).rect();
    let pos = egui::pos2(bar.left() - 20.0, bar.center().y);
    h.event(egui::Event::PointerMoved(pos));
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::default() });
        h.run_steps(1);
    }
    h.run_steps(2);
}

#[test]
fn moving_an_effect_forgets_the_clicked_row() {
    let mut h = harness();
    let first = h.state().session.active().unwrap().current_slide().unwrap().animations[0].shape.0;
    h.state_mut().session.execute("edit.select", &json!({"ids": [first]})).unwrap();
    for effect in ["pulse", "spin"] {
        h.state_mut().session.execute("animation.add", &json!({"effect": effect, "start": "afterPrevious", "duration": 1000})).unwrap();
    }
    h.run_steps(2);
    // Fade, zoom, pulse, spin: pick the pulse, then move it below the spin.
    click_row(&mut h, 3, egui::PointerButton::Primary);
    click_row(&mut h, 3, egui::PointerButton::Secondary);
    h.get_by_label("Move Down").click();
    h.run_steps(3);
    let effects: Vec<String> = h.state().session.active().unwrap().current_slide().unwrap().animations.iter().map(|a| a.effect.clone()).collect();
    assert_eq!(effects[2..], ["spin", "pulse"]);
    let at = play(&mut h, "▶ Play From");
    assert!(at < 0.1, "back to the shape's first effect, not the spin now in that row: {at}");
}

#[test]
fn play_from_skips_a_triggered_effect() {
    let mut h = harness();
    // The zoomed shape's first effect becomes triggered by the faded shape; its second effect,
    // after the 1 s fade, is still on the click sequence.
    let first = h.state().session.active().unwrap().current_slide().unwrap().animations[0].shape.0;
    h.state_mut().session.execute("animation.timing", &json!({"index": 1, "trigger": first})).unwrap();
    h.state_mut().session.execute("animation.add", &json!({"effect": "pulse", "start": "afterPrevious", "duration": 1000})).unwrap();
    h.run_steps(2);
    let at = play(&mut h, "▶ Play From");
    assert!((1.0..1.1).contains(&at), "from the shape's first effect on the click sequence: {at}");
    // A clicked triggered row falls back the same way.
    click_row(&mut h, 2, egui::PointerButton::Primary);
    let at = play(&mut h, "▶ Play From");
    assert!((1.0..1.1).contains(&at), "{at}");
}

#[test]
fn preview_rejects_a_from_that_is_not_an_effect_index() {
    let mut h = harness();
    for from in [json!("1"), json!(-1), json!(1.5)] {
        assert!(h.state_mut().run("animation.preview", json!({"from": from})).is_err(), "{from}");
        assert!(h.state().anim_preview_report().is_none(), "{from}");
    }
    assert_eq!(h.state_mut().run("animation.preview", json!({"from": 1})).unwrap()["from"], 1.0);
}

#[test]
#[ignore = "writes a screenshot for review"]
fn screenshot() {
    let _gpu = crate::gpu_lock::gpu_lock();
    let mut h = harness();
    h.run_steps(2);
    let img = h.render().unwrap();
    img.save("/tmp/animpane.png").unwrap();
}
