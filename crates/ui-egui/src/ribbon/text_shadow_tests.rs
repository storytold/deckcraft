//! Home ▸ Font ▸ Text Shadow (#1662): the button sits in the Font group, is lit by a shadow the
//! text inherits, and turns that shadow off and on.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use deckcraft_model::ShapeId;
use deckcraft_model::text::{LevelStyle, RunProps};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use super::fmt_state;
use crate::{Services, SlideApp};

/// A selected text box whose list style gives its text a shadow (`a:lstStyle`), on the Home tab.
fn harness() -> (Harness<'static, SlideApp>, ShapeId) {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100.0, 100.0, 300.0, 80.0], "text": "Shadowed"})).unwrap();
    let id = ShapeId(r["id"].as_u64().unwrap() as u32);
    s.execute("format.textShadow", &json!({"on": true})).unwrap();
    // Move the shadow from the run to the text box's list style, so the run only inherits it.
    let body = s.doc().unwrap().shape(id).unwrap().text.clone().unwrap();
    let shadow = body.paragraphs[0].runs[0].props.shadow.clone();
    assert!(shadow.is_some());
    s.execute("edit.undo", &json!({})).unwrap();
    s.edit(|doc, sel| {
        let sh = deckcraft_model::find_shape_mut(deckcraft_engine::shapes_mut(doc, sel).unwrap(), id).unwrap();
        let level = LevelStyle { run: RunProps { shadow, ..Default::default() }, ..Default::default() };
        sh.text.as_mut().unwrap().list_style.levels = vec![Some(level)];
        Ok(())
    })
    .unwrap();
    s.execute("edit.select", &json!({"ids": [id.0]})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.tab = "Home".into();
    let mut h = Harness::builder().with_size(egui::vec2(1800.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    (h, id)
}

fn run_props(h: &Harness<'_, SlideApp>, id: ShapeId) -> RunProps {
    h.state().session.active().unwrap().shape(id).unwrap().text.clone().unwrap().paragraphs[0].runs[0].props.clone()
}

fn lit(h: &mut Harness<'_, SlideApp>) -> bool {
    fmt_state(h.state_mut())["shadow"] == true
}

#[test]
fn text_shadow_button_shows_and_toggles_an_inherited_shadow() {
    let (mut h, id) = harness();
    assert!(lit(&mut h), "the inherited shadow lights the button");
    assert!(run_props(&h, id).shadow.is_none());
    h.get_by_label("Text Shadow").click();
    h.run_steps(2);
    assert!(!lit(&mut h), "one click turns the inherited shadow off");
    assert_eq!(run_props(&h, id).own_effects, Some(true));
    h.get_by_label("Text Shadow").click();
    h.run_steps(2);
    assert!(lit(&mut h));
    assert!(run_props(&h, id).shadow.is_some(), "the second click sets the run's own shadow");
}
