//! Modal dialogs open centered and the title bar drags them (deckcraft#1551).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use deckcraft_ui_egui::dialogs::Dialog;
use deckcraft_ui_egui::{Services, SlideApp};
use egui::{Modifiers, PointerButton, Pos2, Rect};
use egui_kittest::Harness;

/// The rectangle of the dialog window (the only movable area on screen).
fn dialog_rect(h: &Harness<'_, SlideApp>) -> Rect {
    h.ctx.memory(|m| {
        m.areas()
            .visible_layer_ids()
            .into_iter()
            .filter(|l| l.order == egui::Order::Middle)
            .filter_map(|l| m.area_rect(l.id))
            .next()
            .expect("dialog window is visible")
    })
}

fn drag(h: &mut Harness<'_, SlideApp>, a: Pos2, b: Pos2) {
    h.hover_at(a);
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    for i in 1..=8 {
        h.event(egui::Event::PointerMoved(a + (b - a) * (i as f32 / 8.0)));
        h.run_steps(1);
    }
    h.event(egui::Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

#[test]
fn dialog_opens_centered_and_follows_title_bar_drag() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.dialog = Some(Dialog::new("renameShape"));
    let mut h = Harness::builder().with_size(egui::vec2(1100.0, 700.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);

    let before = dialog_rect(&h);
    assert!((before.center().x - 550.0).abs() < 2.0, "opens centered horizontally: {before:?}");
    assert!(before.center().y < 350.0 + 1.0 && before.center().y > 250.0, "opens near the vertical center: {before:?}");

    let grab = Pos2::new(before.center().x, before.top() + 10.0);
    drag(&mut h, grab, grab + egui::vec2(-200.0, 120.0));

    let after = dialog_rect(&h);
    assert!(
        (after.min.x - (before.min.x - 200.0)).abs() < 4.0 && (after.min.y - (before.min.y + 120.0)).abs() < 4.0,
        "dialog should follow the drag: before {before:?}, after {after:?}"
    );
    assert_eq!(before.size(), after.size(), "dialog keeps its size");

    // It stays where it was dropped on later frames.
    h.run_steps(3);
    assert_eq!(dialog_rect(&h), after);
}
