//! Design ▸ Effects: the menu applies a theme effect scheme.

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::{Services, SlideApp};

fn harness() -> Harness<'static, SlideApp> {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.ui.tab = "Design".into();
    let mut h = Harness::builder().with_size(egui::vec2(1800.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn format_name(h: &Harness<'_, SlideApp>) -> String {
    h.state().session.active().unwrap().doc.masters[0].theme.format.name.clone()
}

#[test]
fn effects_menu_applies_a_scheme_once() {
    let mut h = harness();
    assert_eq!(format_name(&h), "DeckCraft");
    let n = h.state().session.active().unwrap().history.undo.len();
    h.get_by_label("Effects").click();
    h.run_steps(2);
    h.get_by_label("Soft").click();
    h.run_steps(2);
    assert_eq!(format_name(&h), "Soft");
    assert_eq!(h.state().session.active().unwrap().history.undo.len(), n + 1);
    assert_eq!(h.state().session.active().unwrap().doc.masters[0].theme.name, "DeckCraft");

    h.get_by_label("Effects").click();
    h.run_steps(2);
    h.get_by_label("Soft").click();
    h.run_steps(2);
    assert_eq!(h.state().session.active().unwrap().history.undo.len(), n + 1, "the same scheme is not an undo step");
}
