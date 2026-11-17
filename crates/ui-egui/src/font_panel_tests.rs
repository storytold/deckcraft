//! Font… opens the host's system font panel when it has one (`Services::show_fonts`), on the
//! selection's font; the ribbon's Font… button still opens the Font dialog.

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
fn ribbon_font_button_still_opens_the_dialog() {
    let (mut h, shown) = harness();
    h.get_by_label("Font…").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("font"));
    assert!(shown.borrow().is_empty());
    // The next Font… (⌘T, the menus) goes to the panel again.
    h.state_mut().dialog = None;
    h.state_mut().session.execute("format.fontDialog", &json!({})).unwrap();
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    assert_eq!(shown.borrow().len(), 1);
}
