//! The SmartArt Text Pane driven like a person would: type in a line, Enter for a new line, Tab
//! to demote.

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

use crate::{Services, SlideApp};

fn levels(h: &Harness<'_, SlideApp>) -> Vec<(String, u8)> {
    let st = h.state().session.active().unwrap();
    let (_, d) = deckcraft_engine::cmd::smartart::selected(st).unwrap();
    d.nodes.into_iter().map(|n| (n.text, n.level)).collect()
}

fn line<'a>(h: &'a Harness<'_, SlideApp>, v: &'a str) -> egui_kittest::Node<'a> {
    h.get_all_by_value(v).find(|n| n.accesskit_node().role() == egui::accesskit::Role::TextInput).unwrap()
}

#[test]
fn text_pane_types_adds_and_demotes() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("insert.smartArt", &json!({"kind": "list", "items": ["Plan", "Build"]})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.pane = Some("smartart".into());
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    assert!(h.query_by_label("Text Pane").is_some());

    line(&h, "Build").click();
    h.run_steps(2);
    h.key_press(egui::Key::End);
    line(&h, "Build").type_text(" it");
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    assert_eq!(levels(&h), [("Plan".into(), 0), ("Build it".into(), 0), (String::new(), 0)]);

    // The new line has the focus: type, then Tab demotes it under "Build it".
    h.event(egui::Event::Text("Test".into()));
    h.run_steps(2);
    h.key_press(egui::Key::Tab);
    h.run_steps(4);
    assert_eq!(levels(&h), [("Plan".into(), 0), ("Build it".into(), 0), ("Test".into(), 1)]);
}

#[test]
fn home_convert_to_smartart_gallery_converts_the_text() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("insert.textBox", &json!({"text": "Goals\nRevenue\nGrowth"})).unwrap();
    let app = SlideApp::new(s, Services::default());
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.get_by_label("Convert to SmartArt").click();
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Basic Block List").click();
    h.run_steps(3);
    let st = h.state().session.active().unwrap();
    let (_, d) = deckcraft_engine::cmd::smartart::selected(st).expect("the text became a SmartArt graphic");
    assert_eq!(d.kind, "blockList");
    assert_eq!(d.nodes.len(), 3);
}

#[test]
fn layout_gallery_shows_cached_pictures_and_previews_on_hover() {
    let _gpu = crate::gpu_lock::gpu_lock();
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("insert.smartArt", &json!({"kind": "process", "items": ["Plan", "Build", "Ship"]})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.tab = "SmartArt Design".into();
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.get_by_label("Layouts").click();
    h.run_steps(3);
    let first = h.state().textures.smartart_cached();
    assert!(first >= deckcraft_engine::cmd::smartart::LAYOUTS.len().min(8), "gallery pictures were drawn: {first}");
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Vertical Box List").hover();
    h.run_steps(3);
    if let Ok(path) = std::env::var("SMARTART_GALLERY_PNG") {
        h.render().expect("render").save(path).unwrap();
    }
    let hovered = h.state().textures.smartart_cached();
    assert!(hovered > first, "hovering drew a preview of the graphic's own outline");
    h.run_steps(3);
    assert_eq!(h.state().textures.smartart_cached(), hovered, "pictures are cached, not redrawn every frame");
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Vertical Box List").click();
    h.run_steps(3);
    let st = h.state().session.active().unwrap();
    assert_eq!(deckcraft_engine::cmd::smartart::selected(st).unwrap().1.kind, "verticalBoxList");
}
