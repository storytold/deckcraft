//! The canvas shell through real egui input: switching layouts, what each one shows, the
//! inspector tabs and the toolbar's tools.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::{Session, ToolKind};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, Shell, SlideApp};

fn harness(shell: Shell) -> Harness<'static, SlideApp> {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.run("view.shell", json!({"shell": shell.id()})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    // The app keeps asking for frames, so step instead of run().
    h.run_steps(3);
    h
}

#[test]
fn view_shell_switches_layouts_and_rejects_unknown_ones() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    assert_eq!(app.ui.shell, Shell::Classic, "the ribbon stays the default");
    assert_eq!(app.run("view.shell", json!({"shell": "canvas"})).unwrap()["shell"], "canvas");
    assert_eq!(app.run("view.shell", json!({})).unwrap()["shell"], "classic", "no shell toggles");
    assert_eq!(app.run("view.shell", json!({})).unwrap()["shell"], "canvas");
    assert!(app.run("view.shell", json!({"shell": "figma"})).is_err());
    assert_eq!(app.ui.shell, Shell::Canvas, "a bad name changes nothing");
}

#[test]
fn the_layout_choice_is_kept_with_the_ui_state() {
    let ui = crate::UiState { shell: Shell::Canvas, ..Default::default() };
    let back: crate::UiState = serde_json::from_value(serde_json::to_value(&ui).unwrap()).unwrap();
    assert_eq!(back.shell, Shell::Canvas);
    let old: crate::UiState = serde_json::from_value(json!({"brightness": "dark"})).unwrap();
    assert_eq!(old.shell, Shell::Classic, "settings saved before the canvas shell open in the ribbon");
}

#[test]
fn the_canvas_shell_replaces_the_ribbon_with_sidebar_inspector_and_toolbar() {
    let h = harness(Shell::Canvas);
    assert!(h.query_by_label("File").is_none(), "no ribbon tabs");
    for label in ["Layers", "Design", "Animate", "Export", "Select (V)", "Text box (T)"] {
        assert!(h.query_all_by_label(label).next().is_some(), "`{label}` is shown");
    }
    assert!(h.state().slide_rect.is_some(), "the slide is on the canvas");

    let h = harness(Shell::Classic);
    assert!(h.query_by_label("File").is_some(), "the ribbon is back");
    assert!(h.query_by_label("Layers").is_none());
}

#[test]
fn switching_layouts_leaves_the_presentation_alone() {
    let mut h = harness(Shell::Canvas);
    let before = h.state().session.active().unwrap().doc.clone();
    h.state_mut().run("view.shell", json!({"shell": "classic"})).unwrap();
    h.run_steps(2);
    h.state_mut().run("view.shell", json!({"shell": "canvas"})).unwrap();
    h.run_steps(2);
    let st = h.state().session.active().unwrap();
    assert!(std::sync::Arc::ptr_eq(&before, &st.doc), "no edit is made");
    assert!(!st.is_dirty());
}

#[test]
fn inspector_tabs_switch_between_design_and_animate() {
    let mut h = harness(Shell::Canvas);
    assert_eq!(h.state().ui.inspector_tab, "design");
    h.get_by_label("Animate").click();
    h.run_steps(2);
    assert_eq!(h.state().ui.inspector_tab, "animate");
    h.get_by_label("Design").click();
    h.run_steps(2);
    assert_eq!(h.state().ui.inspector_tab, "design");
}

#[test]
fn toolbar_picks_the_drawing_tool() {
    let mut h = harness(Shell::Canvas);
    h.get_by_label("Text box (T)").click();
    h.run_steps(2);
    assert_eq!(h.state().session.tool.kind, ToolKind::TextBox);
    h.get_by_label("Select (V)").click();
    h.run_steps(2);
    assert_eq!(h.state().session.tool.kind, ToolKind::Select);
}

#[test]
fn the_canvas_shell_draws_in_dark_mode_and_with_nothing_open() {
    let mut h = harness(Shell::Canvas);
    h.state_mut().run("view.dark", json!({"on": true})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Layers").is_some());
    h.state_mut().run("app.closePresentation", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.state().session.active().is_none());
}

fn press(h: &mut Harness<'static, SlideApp>, key: egui::Key, text: &str) {
    h.key_press(key);
    if !text.is_empty() {
        h.event(egui::Event::Text(text.into()));
    }
    h.run_steps(2);
}

#[test]
fn single_keys_pick_tools_in_the_canvas_shell() {
    let mut h = harness(Shell::Canvas);
    press(&mut h, egui::Key::T, "t");
    assert_eq!(h.state().session.tool.kind, ToolKind::TextBox);
    press(&mut h, egui::Key::R, "r");
    assert_eq!(h.state().session.tool.kind, ToolKind::Shape { preset: "rect".into() });
    press(&mut h, egui::Key::O, "o");
    assert_eq!(h.state().session.tool.kind, ToolKind::Shape { preset: "ellipse".into() });
    press(&mut h, egui::Key::Escape, "");
    assert_eq!(h.state().session.tool.kind, ToolKind::Select);
}

#[test]
fn letters_pick_tools_instead_of_typing_over_a_selected_shape() {
    let mut h = harness(Shell::Canvas);
    let app = h.state_mut();
    let id = app.session.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100], "text": "Keep"})).unwrap()["id"]
        .as_u64()
        .unwrap();
    app.session.execute("selection.select", &json!({"ids": [id]})).unwrap();
    h.run_steps(2);
    press(&mut h, egui::Key::T, "t");
    assert_eq!(h.state().session.tool.kind, ToolKind::TextBox);
    let text = h.state_mut().session.execute("text.get", &json!({"id": id})).unwrap();
    assert_eq!(text["text"], "Keep", "the shape's text is untouched");
}

#[test]
fn letters_type_while_editing_text_and_in_the_ribbon() {
    let mut h = harness(Shell::Canvas);
    let app = h.state_mut();
    let id =
        app.session.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100], "text": "A"})).unwrap()["id"].as_u64().unwrap();
    app.session.execute("selection.select", &json!({"ids": [id]})).unwrap();
    app.session.key("Enter", deckcraft_engine::Mods::default()).unwrap();
    app.session.execute("text.move", &json!({"to": "end"})).unwrap();
    h.run_steps(2);
    press(&mut h, egui::Key::T, "t");
    assert_eq!(h.state().session.tool.kind, ToolKind::Select, "no tool while editing");
    let text = h.state_mut().session.execute("text.get", &json!({"id": id})).unwrap();
    assert_eq!(text["text"], "At");

    let mut h = harness(Shell::Classic);
    press(&mut h, egui::Key::T, "t");
    assert_eq!(h.state().session.tool.kind, ToolKind::Select, "the ribbon has no single-key tools");
}

/// Click at slide point (x, y) with the Select tool.
fn click(app: &mut SlideApp, x: f64, y: f64) {
    for kind in ["down", "up"] {
        let ev: deckcraft_engine::PointerEvent = serde_json::from_value(json!({"kind": kind, "x": x, "y": y})).unwrap();
        app.session.pointer(ev).unwrap();
    }
}

#[test]
fn locked_layers_are_selected_from_the_layers_list_not_the_canvas() {
    for shell in [Shell::Canvas, Shell::Classic] {
        let mut app = SlideApp::new(Session::with_new(), Services::default());
        app.run("view.shell", json!({"shell": shell.id()})).unwrap();
        let back = app.session.execute("shape.insert", &json!({"preset": "rect", "rect": [600, 300, 200, 150]})).unwrap()["id"].as_u64().unwrap();
        app.session.execute("shape.lock", &json!({"ids": [back], "locked": true})).unwrap();
        app.session.execute("edit.deselect", &json!({})).unwrap();
        click(&mut app, 700.0, 375.0);
        let picked = app.session.active().unwrap().selection.shapes.iter().any(|i| i.0 as u64 == back);
        match shell {
            Shell::Canvas => assert!(!picked, "the click passes through the locked layer"),
            Shell::Classic => assert!(picked, "PowerPoint's lock: picked, but it doesn't move"),
        }
        // The Layers list (the Selection pane's rows) selects it either way.
        app.session.execute("selection.select", &json!({"ids": [back]})).unwrap();
        assert_eq!(app.session.active().unwrap().selection.shapes.len(), 1);
    }
}

#[test]
fn the_canvas_layout_shows_pixels_and_the_ribbon_points() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    assert_eq!(crate::units::small(&app), crate::units::PT);
    assert_eq!(crate::units::length(&app), crate::units::INCH);
    app.run("view.shell", json!({"shell": "canvas"})).unwrap();
    assert_eq!(crate::units::small(&app), crate::units::PX);
    assert_eq!(crate::units::length(&app), crate::units::PX);
    // Zoom: 100% is one slide pixel per screen point, so a fitted slide at scale 0.9 shows 45%.
    assert_eq!(crate::units::zoom_shown(&app, 0.9), 0.45);
    assert_eq!(crate::units::zoom_percent(&app, 50.0), 100.0);
}

#[test]
fn the_inspector_shows_a_title_size_in_pixels() {
    let mut h = harness(Shell::Canvas);
    let app = h.state_mut();
    let title = app.session.active().unwrap().current_slide().unwrap().shapes[0].id.0;
    app.session.execute("text.set", &json!({"id": title, "text": "Pixel Grid"})).unwrap();
    app.session.execute("selection.select", &json!({"ids": [title]})).unwrap();
    app.ui.zoom = Some(0.9);
    h.run_steps(4);
    // Detached from its style, the title shows its type controls, keeping its size: the title
    // slide's title is 60 pt, 120 px.
    h.get_by_label("Detach style").click();
    h.run_steps(3);
    assert!(h.query_all_by_value("120 px").next().is_some(), "the size field reads 120 px");
    assert!(h.query_by_label("45%").is_some(), "the zoom reads 45%");
}
