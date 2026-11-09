//! Design › Browse for Themes…: picking a .pptx applies its theme.

use std::cell::RefCell;
use std::rc::Rc;

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

#[test]
fn browse_button_applies_the_picked_files_theme() {
    let dir = std::env::temp_dir().join(format!("deckcraft-thmx-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut brand = Session::with_new();
    brand.execute("design.fonts", &json!({"major": "Lato", "minor": "Lato"})).unwrap();
    let path = dir.join("Brand.pptx");
    brand.execute("file.saveAs", &json!({"path": path.to_string_lossy()})).unwrap();
    let asked: Rc<RefCell<Vec<String>>> = Rc::default();
    let log = asked.clone();
    let p2 = path.to_string_lossy().to_string();
    let services = Services {
        pick_open: Some(Box::new(move |purpose: &str| {
            log.borrow_mut().push(purpose.to_string());
            Some(p2.clone())
        })),
        read: Some(Box::new(|p: &str| std::fs::read(p).map_err(|e| e.to_string()))),
        ..Default::default()
    };
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "title", "title": "Hello"})).unwrap();
    let mut app = SlideApp::new(s, services);
    app.ui.tab = "Design".into();
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.get_by_label("Browse for Themes…").click();
    h.run_steps(3);
    assert_eq!(asked.borrow().as_slice(), ["theme"]);
    assert_eq!(h.state().session.active().unwrap().doc.masters[0].theme.fonts.major.latin, "Lato");
    let _ = std::fs::remove_dir_all(&dir);
}
