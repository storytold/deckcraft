//! Headless slide-show UI tests (egui_kittest): the on-screen bar and the keep-ink prompt
//! are verified structurally (widget tree) and, when a GPU is available, rendered to
//! `target/show-ui/*.png` for visual inspection.

use deckcraft_engine::Session;
use deckcraft_ui_egui::{Services, SlideApp};
use egui_kittest::kittest::Queryable;

type Harness = egui_kittest::Harness<'static, SlideApp>;

fn make_show() -> Harness {
    let mut app = SlideApp::new(Session::new(), Services::default());
    deckcraft_engine::sample::open_sample(&mut app.session).expect("sample deck");
    app.ui.presenter_view = false;
    app.start_show(0, false);
    let builder = egui_kittest::Harness::builder().with_size(egui::vec2(1280.0, 800.0)).with_max_steps(8);
    // wgpu rendering when a GPU adapter is available; otherwise structure-only.
    let builder = match std::panic::catch_unwind(egui_kittest::wgpu::WgpuTestRenderer::new) {
        Ok(r) => builder.renderer(r),
        Err(_) => builder,
    };
    let mut h = builder.build_ui_state(
        move |ui, app: &mut SlideApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn key(h: &mut Harness, k: egui::Key, command: bool) {
    let mods = if command { egui::Modifiers::COMMAND } else { egui::Modifiers::NONE };
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::Key { key: k, physical_key: None, pressed, repeat: false, modifiers: mods });
        h.step();
    }
}

fn drag(h: &mut Harness, from: (f32, f32), to: (f32, f32)) {
    h.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(from.0, from.1)));
    h.step();
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(from.0, from.1),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    let steps = 12;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        h.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)));
        h.step();
    }
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(to.0, to.1),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
}

fn click(h: &mut Harness, at: (f32, f32)) {
    h.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(at.0, at.1)));
    h.step();
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: egui::pos2(at.0, at.1),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
}

fn dump(h: &mut Harness, name: &str) {
    if let Ok(img) = h.render() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/show-ui");
        let _ = std::fs::create_dir_all(dir);
        let _ = img.save(format!("{dir}/{name}.png"));
    }
}

/// The band between the tool buttons and Undo holds the swatches and width dots.
fn extras_band(h: &Harness) -> Option<egui::Rect> {
    let l = h.get_by_label("Laser pointer").rect();
    let undo = h.get_by_label("Undo").rect();
    (undo.left() > l.right()).then_some(egui::Rect::from_min_max(egui::pos2(l.right(), l.top()), egui::pos2(undo.left(), l.bottom())))
}

#[test]
fn show_bar_shows_all_tools() {
    let mut h = make_show();
    h.run_steps(2);
    // Every control exists in the widget tree.
    for label in ["Select", "Pen", "Highlighter", "Eraser", "Laser pointer", "Undo", "Clear", "Blank screen", "End"] {
        h.get_by_label(label);
    }
    dump(&mut h, "bar");
    if let Ok(img) = h.render() {
        let band = extras_band(&h).expect("extras between L and Undo");
        // The red swatch is actually painted (not just laid out).
        let mut red = false;
        let mut white = false;
        for y in band.top() as u32..band.bottom() as u32 {
            for x in band.left() as u32..band.right() as u32 {
                let p = img.get_pixel(x, y).0;
                if (p[0] as i32 - 0xE5).abs() < 12 && (p[1] as i32 - 0x39).abs() < 12 && (p[2] as i32 - 0x35).abs() < 12 {
                    red = true;
                }
                if p[0] > 250 && p[1] > 250 && p[2] > 250 {
                    white = true;
                }
            }
        }
        assert!(red, "no red swatch painted in the bar");
        assert!(white, "no selected width dot / white swatch painted in the bar");
    }
}

#[test]
fn undo_clear_disabled_state_tracks_ink() {
    use egui_kittest::kittest::NodeT;
    let mut h = make_show();
    h.run_steps(2);
    let disabled = |h: &Harness, label: &str| h.get_by_label(label).accesskit_node().is_disabled();
    assert!(disabled(&h, "Undo"), "nothing to undo on a fresh show");
    assert!(disabled(&h, "Clear"), "no ink to clear on a fresh show");
    key(&mut h, egui::Key::P, true);
    drag(&mut h, (300.0, 400.0), (900.0, 500.0));
    h.run_steps(2);
    assert!(!disabled(&h, "Undo"), "a drawn stroke can be undone");
    assert!(!disabled(&h, "Clear"), "a drawn stroke can be cleared");
    h.get_by_label("Clear").click();
    h.run_steps(2);
    assert!(!disabled(&h, "Undo"), "the clear itself is undoable");
    assert!(disabled(&h, "Clear"), "no ink left to clear");
    h.get_by_label("Undo").click();
    h.run_steps(2);
    assert!(!disabled(&h, "Clear"), "undo restored the stroke");
}

#[test]
fn pen_draws_and_keep_prompt_has_buttons() {
    let mut h = make_show();
    h.run_steps(2);
    key(&mut h, egui::Key::P, true);
    drag(&mut h, (300.0, 400.0), (900.0, 500.0));
    let showing = format!("{:?}", h.state().show.as_ref().map(|s| s.ink().count()));
    assert!(h.state().show.as_ref().is_some_and(|s| s.ink().count() == 1), "stroke drawn: {showing}");
    dump(&mut h, "ink");
    key(&mut h, egui::Key::Escape, false);
    h.run_steps(2);
    // The keep prompt is up and its buttons exist, are painted, and are clickable.
    h.get_by_label("Keep");
    h.get_by_label("Discard");
    if let Ok(img) = h.render() {
        let keep = h.get_by_label("Keep").rect();
        let (mut fill, mut total) = (0, 0);
        for y in keep.top() as u32..keep.bottom() as u32 {
            for x in keep.left() as u32..keep.right() as u32 {
                total += 1;
                if img.get_pixel(x, y).0 == [70, 70, 70, 255] {
                    fill += 1;
                }
            }
        }
        assert!(total > 0 && fill * 2 > total, "Keep button is laid out but not painted (fill {fill}/{total})");
    }
    dump(&mut h, "keep_prompt");
    h.get_by_label("Keep").click();
    h.run_steps(2);
    let doc_has_ink = h
        .state()
        .session
        .active()
        .is_some_and(|d| d.doc.slides.iter().flat_map(|s| s.shapes.iter()).any(|sh| matches!(sh.kind, deckcraft_model::ShapeKind::Ink { .. })));
    assert!(doc_has_ink, "kept ink should land in the document");
}

#[test]
fn select_tool_selects_moves_duplicates_and_deletes() {
    let mut h = make_show();
    h.run_steps(2);
    key(&mut h, egui::Key::P, true);
    drag(&mut h, (300.0, 400.0), (900.0, 500.0));
    let ink_of = |h: &Harness| h.state().show.as_ref().map(|s| s.ink().clone()).expect("show running");
    assert_eq!(ink_of(&h).count(), 1, "stroke drawn");
    let before = ink_of(&h).strokes(0)[0].points.clone();
    // V switches to the select tool; a click on the stroke selects it (bounding box on).
    key(&mut h, egui::Key::V, false);
    assert_eq!(ink_of(&h).tool().name(), "select");
    click(&mut h, (600.0, 450.0));
    assert_eq!(ink_of(&h).selection_len(), 1, "click on the stroke selects it");
    dump(&mut h, "select");
    // Dragging the selected stroke moves it; the click must not have advanced the show.
    assert_eq!(h.state().show.as_ref().map(|s| s.state.slide), Some(0));
    drag(&mut h, (600.0, 450.0), (660.0, 480.0));
    let moved = ink_of(&h).strokes(0)[0].points.clone();
    let (dx, dy) = (moved[0].0 - before[0].0, moved[0].1 - before[0].1);
    assert!(dx > 0.02 && dy > 0.01, "selection moved with the drag: ({dx}, {dy})");
    // Cmd+D duplicates at an offset; the copy becomes the selection.
    key(&mut h, egui::Key::D, true);
    let ink = ink_of(&h);
    assert_eq!(ink.count(), 2, "duplicated");
    assert_eq!(ink.selection_len(), 1);
    // Delete removes the selection; Cmd+Z restores it.
    key(&mut h, egui::Key::Delete, false);
    assert_eq!(ink_of(&h).count(), 1, "selection deleted");
    key(&mut h, egui::Key::Z, true);
    assert_eq!(ink_of(&h).count(), 2, "delete is undoable");
    // A click on empty space clears the selection.
    click(&mut h, (80.0, 200.0));
    assert_eq!(ink_of(&h).selection_len(), 0, "click on empty space deselects");
}

#[test]
fn navigation_mid_move_commits_the_gesture_on_its_own_slide() {
    let mut h = make_show();
    h.run_steps(2);
    key(&mut h, egui::Key::P, true);
    drag(&mut h, (300.0, 400.0), (900.0, 500.0));
    key(&mut h, egui::Key::V, false);
    let orig = h.state().show.as_ref().map(|s| s.ink().strokes(0)[0].points.clone()).expect("stroke");
    // Press on the stroke and drag a short way.
    h.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(600.0, 450.0)));
    h.step();
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(600.0, 450.0),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    for i in 1..=4 {
        h.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(600.0 + 10.0 * i as f32, 450.0)));
        h.step();
    }
    // Navigate mid-drag, keep dragging, release.
    key(&mut h, egui::Key::ArrowRight, false);
    h.run_steps(2);
    assert_eq!(h.state().show.as_ref().map(|s| s.state.slide), Some(1), "show advanced mid-drag");
    for i in 5..=8 {
        h.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(600.0 + 10.0 * i as f32, 450.0)));
        h.step();
    }
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(680.0, 450.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(2);
    // The stroke moved by exactly the pre-navigation delta (+40px of 1280 → 0.03125).
    let moved = h.state().show.as_ref().map(|s| s.ink().strokes(0)[0].points.clone()).expect("stroke");
    let dx = moved[0].0 - orig[0].0;
    assert!((dx - 40.0 / 1280.0).abs() < 0.01, "only pre-navigation deltas applied: dx {dx}");
    // One undo restores the original position (the move was journaled against slide 0).
    key(&mut h, egui::Key::Z, true);
    h.run_steps(2);
    let restored = h.state().show.as_ref().map(|s| s.ink().strokes(0)[0].points.clone()).expect("stroke");
    assert!((restored[0].0 - orig[0].0).abs() < 1e-4, "undo reverted the move: {:?}", restored[0]);
}

#[test]
fn bar_navigation_drops_the_selection_before_the_next_key() {
    let mut h = make_show();
    h.run_steps(2);
    key(&mut h, egui::Key::P, true);
    drag(&mut h, (300.0, 400.0), (900.0, 500.0));
    key(&mut h, egui::Key::V, false);
    click(&mut h, (600.0, 450.0));
    assert_eq!(h.state().show.as_ref().map(|s| s.ink().selection_len()), Some(1));
    // Navigate with the bar button (applied late in the frame), then hit Delete at once:
    // the selection was dropped with the navigation, so slide 0's ink must survive.
    h.get_by_label("Next slide").click();
    h.run_steps(2);
    assert_eq!(h.state().show.as_ref().map(|s| s.state.slide), Some(1));
    key(&mut h, egui::Key::Delete, false);
    h.run_steps(2);
    let ink = h.state().show.as_ref().map(|s| s.ink().clone()).expect("show running");
    assert_eq!(ink.count(), 1, "Delete after navigation must not touch slide 0's ink");
    assert_eq!(ink.selection_len(), 0);
}
