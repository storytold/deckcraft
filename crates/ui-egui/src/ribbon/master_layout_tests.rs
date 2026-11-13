//! Slide Master ribbon: Preserve, Title and Footers on the open layout.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use deckcraft_model::PhType;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

fn harness(app: SlideApp) -> Harness<'static, SlideApp> {
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

fn app_on_blank() -> (SlideApp, usize) {
    let mut session = Session::with_new();
    let blank = session.doc().unwrap().doc.masters[0].layouts.iter().position(|l| l.name == "Blank").unwrap();
    session.execute("view.slideMaster", &json!({"master": 0, "layout": blank})).unwrap();
    let mut app = SlideApp::new(session, Services::default());
    app.ui.tab = "Slide Master".into();
    (app, blank)
}

fn click_check(h: &mut Harness<'_, SlideApp>, label: &str) {
    h.query_all_by_role_and_label(Role::CheckBox, label).next().unwrap_or_else(|| panic!("no {label} checkbox")).click();
    h.run_steps(3);
}

#[test]
fn slide_master_preserve_title_and_footers() {
    let (app, blank) = app_on_blank();
    let mut h = harness(app);
    assert!(h.query_all_by_role_and_label(Role::CheckBox, "Preserve").next().is_some());
    assert!(h.query_all_by_role_and_label(Role::CheckBox, "Title").next().is_some());
    assert!(h.query_all_by_role_and_label(Role::CheckBox, "Footers").next().is_some());
    {
        let st = h.state().session.doc().unwrap();
        assert!(st.doc.masters[0].layouts[blank].preserve);
        assert!(!st.shapes().iter().any(|sh| sh.ph_type().is_some_and(PhType::is_title)));
        assert!(st.shapes().iter().any(|sh| sh.ph_type().is_some_and(PhType::is_footer_kind)));
    }

    click_check(&mut h, "Title");
    assert!(h.state().session.doc().unwrap().shapes().iter().any(|sh| sh.ph_type().is_some_and(PhType::is_title)));

    click_check(&mut h, "Footers");
    assert!(!h.state().session.doc().unwrap().shapes().iter().any(|sh| sh.ph_type().is_some_and(PhType::is_footer_kind)));

    click_check(&mut h, "Preserve");
    assert!(!h.state().session.doc().unwrap().doc.masters[0].layouts[blank].preserve);
}
