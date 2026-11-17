//! Font… opens the host's system font panel when it has one (`Services::show_fonts`), on the
//! selection's font, from the ribbon too. The ribbon itself sets what the panel doesn't: character
//! spacing, offset and caps.

use std::cell::RefCell;
use std::rc::Rc;

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

type Shown = Rc<RefCell<Vec<(String, f64, bool, bool)>>>;

fn harness() -> (Harness<'static, SlideApp>, Shown) {
    let shown = Shown::default();
    let log = shown.clone();
    let services = Services { show_fonts: Some(Box::new(move |f: &str, s, b, i| log.borrow_mut().push((f.into(), s, b, i)))), ..Default::default() };
    let mut app = SlideApp::new(Session::with_new(), services);
    app.session.execute("shape.insert", &json!({"preset": "rect", "rect": [40.0, 40.0, 240.0, 80.0], "text": "Hello"})).unwrap();
    app.session.execute("format.italic", &json!({"on": true})).unwrap();
    app.session.execute("format.fontDialog", &json!({"size": 24})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1800.0, 1100.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    (h, shown)
}

#[test]
fn font_command_shows_the_system_panel_on_the_selections_font() {
    let (mut h, shown) = harness();
    h.state_mut().session.execute("format.fontDialog", &json!({})).unwrap();
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    let shown = shown.borrow();
    assert_eq!(shown.len(), 1, "{shown:?}");
    let (family, size, bold, italic) = &shown[0];
    assert!(!family.is_empty());
    assert_eq!((*size, *bold, *italic), (24.0, false, true));
}

#[test]
fn ribbon_font_button_shows_the_panel_too() {
    let (mut h, shown) = harness();
    h.get_by_label("Font…").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    assert_eq!(shown.borrow().len(), 1);
}

fn type_into(h: &mut Harness<'_, SlideApp>, field: &str, text: &str) {
    h.get_by_label(field).focus();
    h.run_steps(1);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_label(field).type_text(text);
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
}

#[test]
fn ribbon_sets_spacing_offset_and_caps() {
    let (mut h, _) = harness();
    let state = |h: &mut Harness<'_, SlideApp>| h.state_mut().session.execute("format.state", &json!({})).unwrap();
    h.get_by_label("Character Spacing").click();
    h.run_steps(2);
    type_into(&mut h, "Spacing value", "2.5");
    type_into(&mut h, "Offset value", "-10");
    h.get_by_label("Change Case").click();
    h.run_steps(2);
    h.get_by_label("Small Caps").click();
    h.run_steps(2);
    let st = state(&mut h);
    assert_eq!((st["spacing"].as_f64(), st["baseline"].as_f64(), st["caps"].as_str()), (Some(2.5), Some(-0.1), Some("small")), "{st}");
    assert!(st["subscript"] == true, "a lowered offset reads as subscript: {st}");
    // Small Caps again turns it off.
    h.get_by_label("Change Case").click();
    h.run_steps(2);
    h.get_by_label("Small Caps").click();
    h.run_steps(2);
    assert_eq!(state(&mut h)["caps"], "none");
}
