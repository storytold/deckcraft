//! The ruler's indent and tab markers driven with the mouse while editing text.

use deckcraft_engine::Session;
use egui::{Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use serde_json::json;

use crate::{Services, SlideApp};

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

fn click(h: &mut Harness<'_, SlideApp>, at: Pos2) {
    h.hover_at(at);
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.event(egui::Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

fn ruler(h: &mut Harness<'_, SlideApp>) -> serde_json::Value {
    h.state_mut().session.execute("format.ruler", &json!({})).unwrap()
}

#[test]
fn drag_indents_add_move_and_remove_tabs_on_the_ruler() {
    let _gpu = crate::gpu_lock::gpu_lock();
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let id = s.execute("insert.textBox", &json!({"rect": [100, 150, 500, 40]})).unwrap()["id"].as_u64().unwrap();
    s.execute("text.edit", &json!({"id": id})).unwrap();
    s.execute("text.insert", &json!({"text": "Name\tRole\tTeam"})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.ruler = true;
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    let (slide, k) = (h.state().slide_rect.unwrap(), h.state().canvas_scale);
    let x0 = ruler(&mut h)["x"].as_f64().unwrap() as f32;
    // The horizontal ruler runs along the top of the canvas.
    let top = h.state().canvas_rect.unwrap().min.y;
    let rx = |q: f32| slide.min.x + (x0 + q) * k;

    // First-line indent triangle (top) to 36 pt.
    drag(&mut h, Pos2::new(rx(0.0), top + 3.0), Pos2::new(rx(36.0), top + 3.0));
    let r = ruler(&mut h);
    assert!((r["indent"].as_f64().unwrap() - 36.0).abs() <= 1.0, "{r}");
    assert_eq!(r["marginLeft"].as_f64(), Some(0.0));

    // Hanging triangle (middle) to 18 pt: the first line stays at 36.
    drag(&mut h, Pos2::new(rx(0.0), top + 8.0), Pos2::new(rx(18.0), top + 8.0));
    let r = ruler(&mut h);
    let (ml, ind) = (r["marginLeft"].as_f64().unwrap(), r["indent"].as_f64().unwrap());
    assert!((ml - 18.0).abs() <= 1.0 && (ml + ind - 36.0).abs() <= 1.0, "{r}");

    // Click on the ruler: a left tab; drag it along; drag it off to remove it.
    click(&mut h, Pos2::new(rx(150.0), top + 12.0));
    let r = ruler(&mut h);
    assert_eq!(r["tabs"].as_array().unwrap().len(), 1, "{r}");
    let pos = r["tabs"][0]["pos"].as_f64().unwrap() as f32;
    assert!((pos - 150.0).abs() <= 1.0);
    drag(&mut h, Pos2::new(rx(pos), top + 12.0), Pos2::new(rx(220.0), top + 12.0));
    let pos = ruler(&mut h)["tabs"][0]["pos"].as_f64().unwrap() as f32;
    assert!((pos - 220.0).abs() <= 1.0, "{pos}");
    if let Ok(img) = h.render() {
        let _ = img.save("/tmp/ruler.png");
    }
    assert!(h.state().session.active().unwrap().selection.text.is_some(), "still editing the text");
    drag(&mut h, Pos2::new(rx(pos), top + 12.0), Pos2::new(rx(pos), top + 80.0));
    assert!(ruler(&mut h)["tabs"].as_array().unwrap().is_empty());
}

fn editing_harness() -> Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let id = s.execute("insert.textBox", &json!({"rect": [100, 150, 500, 40]})).unwrap()["id"].as_u64().unwrap();
    s.execute("text.edit", &json!({"id": id})).unwrap();
    s.execute("text.insert", &json!({"text": "Item\t9.50"})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.ruler = true;
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

#[test]
fn the_tab_selector_picks_the_kind_of_stop_a_click_sets() {
    let _gpu = crate::gpu_lock::gpu_lock();
    use egui_kittest::kittest::Queryable;
    let mut h = editing_harness();
    let (slide, k) = (h.state().slide_rect.unwrap(), h.state().canvas_scale);
    let x0 = ruler(&mut h)["x"].as_f64().unwrap() as f32;
    let top = h.state().canvas_rect.unwrap().min.y;
    let rx = |q: f32| slide.min.x + (x0 + q) * k;
    let corner = h.state().canvas_rect.unwrap().min + egui::vec2(8.0, 8.0);
    assert!(h.query_by_label("Left Tab").is_some());
    for _ in 0..3 {
        click(&mut h, corner);
    }
    assert!(h.query_by_label("Decimal Tab").is_some());
    click(&mut h, Pos2::new(rx(200.0), top + 12.0));
    let r = ruler(&mut h);
    assert_eq!(r["tabs"][0]["align"], "dec", "{r}");
    if let Ok(img) = h.render() {
        let _ = img.save("/tmp/ruler_decimal.png");
    }
}

#[test]
fn tabs_dialog_sets_clears_and_applies_on_ok() {
    let _gpu = crate::gpu_lock::gpu_lock();
    use egui_kittest::kittest::Queryable;
    let mut h = editing_harness();
    h.state_mut().session.execute("format.tabs", &json!({"add": {"pos": 36}})).unwrap();
    h.state_mut().dialog = Some(crate::dialogs::Dialog::new("tabs"));
    h.run_steps(3);
    assert!(h.query_by_label_contains("0.50\"").is_some(), "the existing stop is listed");
    let d = h.state_mut().dialog.as_mut().unwrap();
    d.fields.insert("newpos".into(), "2".into());
    d.fields.insert("default".into(), "0.5".into());
    h.run_steps(1);
    h.get_by_label("Center").click();
    h.run_steps(2);
    h.get_by_label("Set").click();
    h.run_steps(2);
    if let Ok(img) = h.render() {
        let _ = img.save("/tmp/tabs_dialog.png");
    }
    // Nothing changes until OK.
    assert_eq!(ruler(&mut h)["tabs"].as_array().unwrap().len(), 1);
    h.query_all_by_label("Clear").next().expect("a Clear button per stop").click();
    h.run_steps(2);
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    let r = ruler(&mut h);
    assert_eq!(r["tabs"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!((r["tabs"][0]["pos"].as_f64(), r["tabs"][0]["align"].as_str()), (Some(144.0), Some("ctr")));
    assert_eq!(r["defaultTab"].as_f64(), Some(36.0));
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(ruler(&mut h)["tabs"][0]["pos"].as_f64(), Some(36.0), "one undo step");
}
