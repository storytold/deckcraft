//! ⌘V when the system clipboard holds no text: egui-winit then drops the press and only the V
//! key's release arrives (see `image_paste` in `SlideApp::keyboard`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use egui_kittest::Harness;
use serde_json::json;

use crate::{Services, SlideApp};

fn png() -> Vec<u8> {
    deckcraft_render::Image { width: 4, height: 4, pixels: vec![200; 64] }.to_png()
}

fn harness(services: Services) -> Harness<'static, SlideApp> {
    let app = SlideApp::new(Session::with_new(), services);
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn v(h: &mut Harness<'static, SlideApp>, pressed: bool, modifiers: egui::Modifiers) {
    h.event(egui::Event::Key { key: egui::Key::V, physical_key: None, pressed, repeat: false, modifiers });
    h.run_steps(2);
}

fn shapes(h: &Harness<'static, SlideApp>) -> usize {
    let d = h.state().session.active().unwrap();
    d.current_slide().unwrap().shapes.len()
}

fn pictures(h: &Harness<'static, SlideApp>) -> usize {
    let d = h.state().session.active().unwrap();
    d.current_slide().unwrap().shapes.iter().filter(|s| matches!(s.kind, deckcraft_model::ShapeKind::Picture { .. })).count()
}

#[test]
fn command_v_with_an_image_on_the_clipboard_inserts_a_picture() {
    let mut h = harness(Services { clipboard_image: Some(Box::new(|| Some(png()))), ..Default::default() });
    assert_eq!(pictures(&h), 0);
    v(&mut h, false, egui::Modifiers::COMMAND);
    assert_eq!(pictures(&h), 1, "the dropped ⌘V press still pastes the image");
}

#[test]
fn command_v_pastes_shapes_copied_here() {
    let mut h = harness(Services::default());
    h.state_mut().run("shape.insert", json!({"preset": "rect", "rect": [100, 100, 200, 100]})).unwrap();
    h.state_mut().session.execute("edit.copy", &json!({})).unwrap();
    let before = shapes(&h);
    v(&mut h, false, egui::Modifiers::COMMAND);
    assert_eq!(shapes(&h), before + 1, "a shape without text pastes");
}

#[test]
fn a_paste_event_or_a_typed_v_doesnt_paste_again() {
    let mut h = harness(Services { clipboard_image: Some(Box::new(|| Some(png()))), ..Default::default() });
    // ⌘V with text on the clipboard: a Paste event, then the release.
    h.event(egui::Event::Paste("hello".into()));
    h.run_steps(2);
    let after_text = shapes(&h);
    v(&mut h, false, egui::Modifiers::COMMAND);
    assert_eq!(shapes(&h), after_text, "the release after a Paste event is not a second paste");
    assert_eq!(pictures(&h), 0);
    // A V typed (press seen), or a release without ⌘ (an input method took the press).
    v(&mut h, true, egui::Modifiers::NONE);
    v(&mut h, false, egui::Modifiers::NONE);
    v(&mut h, false, egui::Modifiers::NONE);
    assert_eq!(pictures(&h), 0, "plain V is never a paste");
}

/// Bold "Hello" copied from one text box, then the caret at the end of another; the pasted runs'
/// bold flags after a Paste event with `mods` held.
fn paste_bold_hello(mods: egui::Modifiers) -> Vec<Option<bool>> {
    let mut h = harness(Services::default());
    let app = h.state_mut();
    let a = app.run("insert.textBox", json!({"rect": [50, 50, 300, 40], "text": "Hello"})).unwrap()["id"].clone();
    app.run("text.edit", json!({"id": a})).unwrap();
    app.run("text.select", json!({"anchor": [0, 0], "caret": [0, 5]})).unwrap();
    app.run("format.bold", json!({"on": true})).unwrap();
    app.session.execute("edit.copy", &json!({})).unwrap();
    let b = app.run("insert.textBox", json!({"rect": [50, 150, 300, 40], "text": "x"})).unwrap()["id"].clone();
    app.run("text.edit", json!({"id": b, "end": true})).unwrap();
    h.event(egui::Event::ModifiersChanged(mods));
    h.event(egui::Event::Paste("Hello".into()));
    h.run_steps(2);
    let d = h.state().session.active().unwrap();
    let id = serde_json::from_value(b).unwrap();
    let body = d.shape(id).unwrap().text.clone().unwrap();
    assert_eq!(body.text(), "xHello");
    body.paragraphs[0].runs.iter().filter(|r| r.text.contains('H')).map(|r| r.props.bold).collect()
}

#[test]
fn shift_command_v_pastes_text_without_its_formatting() {
    assert_eq!(paste_bold_hello(egui::Modifiers::COMMAND), vec![Some(true)], "⌘V keeps the bold");
    let plain = paste_bold_hello(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT);
    assert!(plain.iter().all(|b| *b != Some(true)), "⇧⌘V drops it: {plain:?}");
}
