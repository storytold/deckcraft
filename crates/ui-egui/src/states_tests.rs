//! States: Morph chains, adding a state, what changes between states, and the Animate tab.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use super::*;
use crate::{Services, SlideApp};

/// A one-slide deck with a named box on it, selected.
fn app_with_box() -> (SlideApp, u32) {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    let id = app.session.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 120, 80]})).unwrap()["id"].as_u64().unwrap() as u32;
    app.session.execute("selection.select", &json!({"ids": [id]})).unwrap();
    app.run("shape.rename", json!({"name": "Card"})).unwrap();
    (app, id)
}

fn doc(app: &SlideApp) -> std::sync::Arc<Presentation> {
    app.session.active().unwrap().doc.clone()
}

/// The shape named `name` on slide `i`.
fn named(app: &SlideApp, i: usize, name: &str) -> u32 {
    doc(app).slides[i].shapes.iter().find(|s| s.name == name).unwrap().id.0
}

#[test]
fn adding_a_state_duplicates_the_slide_with_morph_in_one_undo_step() {
    let (mut app, _) = app_with_box();
    let undo0 = app.session.active().unwrap().history.undo.len();
    add_state(&mut app);
    let d = doc(&app);
    assert_eq!(d.slides.len(), 2);
    assert_eq!(app.session.active().unwrap().selection.slide, 1, "the new state is current");
    assert!(is_state_step(&d, 1));
    assert_eq!(run_of(&d, 0), 0..2);
    assert_eq!(run_of(&d, 1), 0..2);
    let st = app.session.active().unwrap();
    assert_eq!(st.history.undo.len(), undo0 + 1);
    assert_eq!(st.history.undo.last().unwrap().label, "Add State");
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(doc(&app).slides.len(), 1, "one undo removes the state");
}

#[test]
fn runs_stop_at_slides_without_morph() {
    let (mut app, _) = app_with_box();
    add_state(&mut app);
    add_state(&mut app);
    app.run("slide.new", json!({})).unwrap();
    let d = doc(&app);
    assert_eq!(run_of(&d, 1), 0..3);
    assert_eq!(run_of(&d, 3), 3..4, "a plain slide is a run of one");
    assert!(!is_state_step(&d, 0) && !is_state_step(&d, 3));
}

#[test]
fn changes_name_what_morph_will_animate() {
    let (mut app, _) = app_with_box();
    app.session.execute("shape.insert", &json!({"preset": "ellipse", "rect": [400, 100, 50, 50]})).unwrap();
    app.run("shape.rename", json!({"name": "Dot"})).unwrap();
    add_state(&mut app);
    let card = named(&app, 1, "Card");
    let dot = named(&app, 1, "Dot");
    app.session.execute("shape.move", &json!({"ids": [card], "x": 300, "y": 200})).unwrap();
    app.session.execute("shape.fill", &json!({"ids": [card], "color": "#FF0000"})).unwrap();
    app.session.execute("selection.select", &json!({"ids": [dot]})).unwrap();
    app.run("edit.delete", json!({})).unwrap();
    app.session.execute("shape.insert", &json!({"preset": "star5", "rect": [50, 300, 60, 60]})).unwrap();
    let d = doc(&app);
    let list = changes(&d.slides[0], &d.slides[1]);
    let of = |name: &str| list.iter().find(|c| c.name == name).map(|c| c.changes.clone());
    assert_eq!(of("Card"), Some(vec![Change::Moved, Change::Restyled]));
    assert_eq!(of("Dot"), Some(vec![Change::Leaves]));
    assert!(list.iter().any(|c| c.changes == [Change::Enters]), "the star fades in");
    // The Layers list marks the card (and the star), not the layer that left.
    app.run("slide.go", json!({"index": 1})).unwrap();
    let marked = changed_layers(&app);
    assert!(marked.contains(&card));
    assert_eq!(marked.len(), 2);
}

#[test]
fn hiding_with_the_eye_comes_with_a_warning() {
    let (mut app, _) = app_with_box();
    add_state(&mut app);
    let card = named(&app, 1, "Card");
    app.session.execute("shape.visible", &json!({"ids": [card], "visible": false})).unwrap();
    let d = doc(&app);
    let list = changes(&d.slides[0], &d.slides[1]);
    assert_eq!(list[0].changes, [Change::Hidden]);
    assert!(list[0].warning().is_some());
}

#[test]
fn states_survive_a_pptx_round_trip() {
    let (mut app, _) = app_with_box();
    add_state(&mut app);
    let card = named(&app, 1, "Card");
    app.session.execute("shape.move", &json!({"ids": [card], "x": 300, "y": 200})).unwrap();
    let data = app.session.execute("file.saveBytes", &json!({"format": "pptx"})).unwrap()["data"].clone();
    app.session.execute("file.openBytes", &json!({"name": "states.pptx", "data": data})).unwrap();
    let d = doc(&app);
    assert_eq!(run_of(&d, 0), 0..2, "the Morph link is kept");
    let list = changes(&d.slides[0], &d.slides[1]);
    assert_eq!(list.iter().find(|c| c.name == "Card").map(|c| c.changes.clone()), Some(vec![Change::Moved]));
}

#[test]
fn the_animate_tab_adds_and_walks_states() {
    let (mut app, _) = app_with_box();
    app.run("view.shell", json!({"shell": "canvas"})).unwrap();
    app.ui.inspector_tab = "animate".into();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.get_by_label("+ Add state").click();
    h.run_steps(4);
    assert_eq!(doc(h.state()).slides.len(), 2);
    assert!(h.query_by_label("State 2").is_some(), "the strip under the slide shows the states");
    assert!(h.query_by_label("Transition from the previous state").is_some());
    h.get_by_label("Words").click();
    h.run_steps(3);
    assert_eq!(doc(h.state()).slides[1].transition.as_ref().unwrap().option, "words");
    h.get_by_label("State 1").click();
    h.run_steps(3);
    assert_eq!(h.state().session.active().unwrap().selection.slide, 0);
}

fn canvas(app: SlideApp) -> Harness<'static, SlideApp> {
    let mut app = app;
    app.run("view.shell", json!({"shell": "canvas"})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn morph(app: &SlideApp, i: usize) -> bool {
    is_state_step(&doc(app), i)
}

#[test]
fn the_strip_adds_states_after_any_state() {
    let (app, _) = app_with_box();
    let mut h = canvas(app);
    assert!(h.query_by_label("State 1").is_none(), "no strip for a slide without states");
    h.get_by_label("Animate").click();
    h.run_steps(3);
    h.get_by_label("Add a state after state 1").click();
    h.run_steps(3);
    h.get_by_label("Add a state after state 1").click();
    h.run_steps(3);
    let d = doc(h.state());
    assert_eq!(d.slides.len(), 3);
    assert_eq!(run_of(&d, 0), 0..3);
    assert_eq!(h.state().session.active().unwrap().selection.slide, 1, "the new state went in after the first");
    assert!(h.query_by_label("Morph into state 2").is_some(), "timing between states");
    h.get_by_label("Design").click();
    h.run_steps(2);
    assert!(h.query_by_label("State 3").is_none(), "the strip is Animate's only, even with states");
    h.get_by_label("Animate").click();
    h.run_steps(2);
    assert!(h.query_by_label("State 3").is_some(), "back with Animate");
}

#[test]
fn moving_and_deleting_the_first_state_keeps_the_run_apart() {
    let (mut app, _) = app_with_box();
    app.run("transition.set", json!({"kind": "fade"})).unwrap();
    // A plain slide before the run.
    app.run("slide.new", json!({})).unwrap();
    app.run("slide.move", json!({"from": 1, "to": 0})).unwrap();
    app.run("slide.go", json!({"index": 1})).unwrap();
    add_state(&mut app);
    add_state(&mut app);
    let d = doc(&app);
    assert_eq!(run_of(&d, 1), 1..4);
    let first = d.slides[1].id;
    move_state(&mut app, 1, true);
    let d = doc(&app);
    assert_eq!(d.slides[2].id, first, "the first state moved one on");
    assert_eq!(d.slides[1].transition.as_ref().unwrap().kind, "fade", "the run's transition stays at its start");
    assert!(morph(&app, 2) && morph(&app, 3));
    assert_eq!(run_of(&d, 1), 1..4);
    delete_state(&mut app, 1);
    let d = doc(&app);
    assert_eq!(d.slides.len(), 3);
    assert!(!morph(&app, 1), "the new first state doesn't join the slide before");
    assert_eq!(run_of(&d, 1), 1..3);
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(doc(&app).slides.len(), 4, "one undo step");
}

#[test]
fn the_slide_list_shows_a_slide_and_its_states_once() {
    let (mut app, _) = app_with_box();
    add_state(&mut app);
    add_state(&mut app);
    app.run("slide.new", json!({})).unwrap();
    let mut h = canvas(app);
    h.state_mut().run("slide.go", json!({"index": 0})).unwrap();
    h.run_steps(3);
    // ↓ goes past the states to the next entry.
    h.state_mut().slides_focus = true;
    assert!(crate::thumbs::slide_key(h.state_mut(), egui::Key::ArrowDown, deckcraft_engine::Mods::default()));
    assert_eq!(h.state().session.active().unwrap().selection.slide, 3);
    // ⌘↑ moves the plain slide before the whole run.
    let cmd = deckcraft_engine::Mods { cmd: true, ..Default::default() };
    assert!(crate::thumbs::slide_key(h.state_mut(), egui::Key::ArrowUp, cmd));
    let d = doc(h.state());
    assert_eq!(run_of(&d, 1), 1..4, "the run moved together");
    assert_eq!(h.state().session.active().unwrap().selection.slide, 0);
    // Deleting the run's entry deletes its states with it.
    h.state_mut().run("slide.go", json!({"index": 2})).unwrap();
    crate::thumbs::select_runs(h.state_mut());
    h.state_mut().run("slide.delete", json!({})).unwrap();
    assert_eq!(doc(h.state()).slides.len(), 1);
}

#[test]
fn block_moves_keep_order() {
    use crate::thumbs::block_moves;
    assert_eq!(block_moves(2..4, 0), vec![(2, 0), (3, 1)]);
    assert_eq!(block_moves(0..2, 5), vec![(0, 4), (0, 4)]);
    assert!(block_moves(1..3, 2).is_empty() && block_moves(1..3, 3).is_empty() && block_moves(1..3, 1).is_empty());
}

#[test]
fn a_text_build_counts_a_click_per_paragraph() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    let id = app.run("insert.textBox", json!({"rect": [50, 50, 300, 100], "text": "One\nTwo\nThree"})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.select", json!({"ids": [id]})).unwrap();
    app.ui.inspector_tab = "animate".into();
    let mut h = canvas(app);
    h.get_by_label("Fade").click();
    h.run_steps(3);
    let d = doc(h.state());
    assert_eq!(build_steps(&d.slides[0]), 3);
    h.get_by_label("None").click();
    h.run_steps(3);
    assert_eq!(build_steps(&doc(h.state()).slides[0]), 0);
}

#[test]
fn preview_plays_the_states_in_the_canvas_and_ends_by_itself() {
    let (mut app, _) = app_with_box();
    add_state(&mut app);
    app.run("slide.new", json!({})).unwrap();
    app.run("slide.go", json!({"index": 0})).unwrap();
    app.ui.inspector_tab = "animate".into();
    let mut h = canvas(app);
    h.get_by_label("▶ Preview").click();
    h.run_steps(3);
    assert!(h.state().previewing(), "playing in the canvas");
    assert!(h.query_by_label("Grid").is_some() && h.query_by_label("Layers").is_some(), "the editor stays around it");
    // It plays on into the second state, then stops before the next slide.
    let mut seen = vec![];
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        h.run_steps(1);
        match h.state().show.as_ref() {
            Some(s) => seen.push(s.state.slide),
            None => break,
        }
    }
    assert!(h.state().show.is_none(), "the preview ended by itself");
    assert!(seen.contains(&1), "it reached the second state: {seen:?}");
    assert!(!seen.contains(&2), "and stopped before the next slide: {seen:?}");
}

#[test]
fn preview_starts_from_the_selected_state() {
    let (mut app, _) = app_with_box();
    add_state(&mut app);
    add_state(&mut app);
    app.run("slide.new", json!({})).unwrap();
    let d = doc(&app);
    assert_eq!(preview_range(&d, 1), (1, 2), "from the second state, the one selected, to the last");
    assert_eq!(preview_range(&d, 0), (0, 2));
    assert_eq!(preview_range(&d, 3), (3, 3), "a slide without states plays only itself");
    // The button plays that range.
    app.run("slide.go", json!({"index": 1})).unwrap();
    app.ui.inspector_tab = "animate".into();
    let mut h = canvas(app);
    h.get_by_label("▶ Preview").click();
    h.run_steps(1);
    assert_eq!(h.state().show.as_ref().and_then(|s| s.stop_after), Some(2));
}

#[test]
fn preview_from_a_state_without_effects_shows_it_morphs_on_and_gives_editing_back() {
    // A state with nothing to click through used to be skipped at once, with no Morph.
    let (mut app, _) = app_with_box();
    add_state(&mut app);
    add_state(&mut app);
    app.run("slide.new", json!({})).unwrap();
    app.run("slide.go", json!({"index": 1})).unwrap();
    app.run("edit.deselect", json!({})).unwrap();
    app.ui.inspector_tab = "animate".into();
    let mut h = canvas(app);
    h.get_by_label("▶ Preview").click();
    let mut seen = vec![];
    for _ in 0..80 {
        h.run_steps(1);
        match h.state().show.as_ref() {
            Some(s) => seen.push(s.state.slide),
            None => break,
        }
    }
    assert!(h.state().show.is_none(), "the preview ended: {seen:?}");
    let first_last = seen.iter().position(|s| *s == 2).unwrap();
    assert!(seen[..first_last].len() >= 2 && seen[..first_last].iter().all(|s| *s == 1), "it held the selected state first: {seen:?}");
    assert!(seen.iter().filter(|s| **s == 2).count() >= 4, "and the last state after its Morph: {seen:?}");
    assert_eq!(h.state().session.active().unwrap().selection.slide, 1, "editing carries on at the state it started from");
}

#[test]
fn a_clicked_state_takes_the_keyboard() {
    let (mut app, _) = app_with_box();
    add_state(&mut app);
    add_state(&mut app);
    app.run("slide.new", json!({})).unwrap();
    app.run("slide.go", json!({"index": 0})).unwrap();
    app.run("edit.deselect", json!({})).unwrap();
    app.ui.inspector_tab = "animate".into();
    let mut h = canvas(app);
    let at = |h: &Harness<'static, SlideApp>| h.state().session.active().unwrap().selection.slide;
    h.get_by_label("State 2").click();
    h.run_steps(2);
    assert!(h.state().states_focus && !h.state().slides_focus);
    assert_eq!(at(&h), 1);
    // → / ← walk the states and stop at the run's ends (not on to the next slide).
    for (key, want) in [(egui::Key::ArrowRight, 2), (egui::Key::ArrowRight, 2), (egui::Key::ArrowLeft, 1), (egui::Key::Home, 0)] {
        h.key_press(key);
        h.run_steps(2);
        assert_eq!(at(&h), want, "{key:?}");
    }
    // ⌘→ moves the state; ⌘D adds one after it; Delete removes it.
    let first = doc(h.state()).slides[0].id;
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::ArrowRight);
    h.run_steps(2);
    assert_eq!(doc(h.state()).slides[1].id, first, "moved one place right");
    assert_eq!(run_of(&doc(h.state()), 0), 0..3, "still one run");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::D);
    h.run_steps(2);
    assert_eq!(run_of(&doc(h.state()), 0), 0..4, "a state added");
    h.key_press(egui::Key::Delete);
    h.run_steps(2);
    assert_eq!(run_of(&doc(h.state()), 0), 0..3, "and deleted");
    assert_eq!(doc(h.state()).slides.len(), 4, "the slide after the run is untouched");
    // Esc, or a click elsewhere, gives the keyboard back.
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(!h.state().states_focus);
    h.get_by_label("State 1").click();
    h.run_steps(2);
    assert!(h.state().states_focus);
    h.get_by_label("Design").click();
    h.run_steps(2);
    assert!(!h.state().states_focus, "a click elsewhere (and the strip hiding with Animate)");
}

#[test]
fn a_dragged_state_drops_at_the_nearest_gap() {
    let r = |x: f32| egui::Rect::from_min_size(egui::pos2(x, 0.0), egui::vec2(80.0, 50.0));
    let rects = [r(0.0), r(100.0), r(200.0), r(300.0)];
    // Dragging the first to past the third: it goes third (after taking it out).
    assert_eq!(drop_at(&rects, 0, 290.0).map(|d| d.0), Some(2));
    // Dragging the last to the front.
    assert_eq!(drop_at(&rects, 3, -5.0).map(|d| d.0), Some(0));
    // Back where it was.
    assert_eq!(drop_at(&rects, 1, 95.0).map(|d| d.0), Some(1));
}

#[test]
fn moving_the_first_state_to_the_end_keeps_the_run_together() {
    let (mut app, _) = app_with_box();
    app.run("transition.set", json!({"kind": "fade"})).unwrap();
    add_state(&mut app);
    add_state(&mut app);
    let first = doc(&app).slides[0].id;
    move_state_to(&mut app, 0, 2);
    let d = doc(&app);
    assert_eq!(d.slides[2].id, first, "the first state is last now");
    assert_eq!(d.slides[0].transition.as_ref().unwrap().kind, "fade", "the run starts with its own transition");
    assert!(is_state_step(&d, 1) && is_state_step(&d, 2), "the rest are reached with Morph");
    assert_eq!(run_of(&d, 0), 0..3);
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(doc(&app).slides[0].id, first, "one undo step");
}
