//! The Design inspector: its sections for each kind of selection, and that its controls run the
//! commands they name.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use super::*;
use crate::{Services, Shell, SlideApp};

fn harness() -> Harness<'static, SlideApp> {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.run("view.shell", json!({"shell": "canvas"})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    assert_eq!(h.state().ui.shell, Shell::Canvas);
    h
}

#[test]
fn nothing_selected_shows_the_slide_layout_and_background() {
    let h = harness();
    assert!(h.query_by_label("Slide").is_some());
    assert!(h.query_by_label("Background").is_some());
    assert!(h.query_by_label("Text").is_none());
}

#[test]
fn weights_are_names_pptx_can_store() {
    // An unknown family offers the bold flag only.
    let (base, styles, cur) = weights("No Such Family", true);
    assert_eq!((base.as_str(), cur.as_str()), ("No Such Family", "Bold"));
    assert_eq!(styles, ["Regular", "Bold"]);
    // Every weight offered for a real family is Regular, Bold or a family name of its own.
    let db = FontDb::global();
    let family = db.families().into_iter().find(|f| db.styles(f).len() > 2).unwrap_or_else(|| "Inter".into());
    let (base, styles, _) = weights(&family, false);
    for s in &styles {
        assert!(s == "Regular" || s == "Bold" || db.has_family(&format!("{base} {s}")), "{base} {s}");
    }
    assert_eq!(weight_commands("Inter", "SemiBold")[0].1["family"], "Inter SemiBold");
    assert_eq!(weight_commands("Inter", "Bold")[1].1["on"], true);
}

#[test]
fn the_text_style_menu_names_the_style_and_applies_another() {
    let mut h = harness();
    let app = h.state_mut();
    let title = app.session.active().unwrap().current_slide().unwrap().shapes[0].id.0;
    // (An empty placeholder keeps its layout's formatting: the engine formats runs.)
    app.session.execute("text.set", &json!({"id": title, "text": "Pixel Grid"})).unwrap();
    app.session.execute("selection.select", &json!({"ids": [title]})).unwrap();
    h.run_steps(3);
    h.get_by_label("Text style: Title").click();
    h.run_steps(3);
    h.get_by_label("Body 3").click();
    h.run_steps(3);
    let st = h.state_mut().session.execute("format.state", &json!({})).unwrap();
    assert_eq!(st["size"], 20.0);
    assert_eq!(st["fontRef"], "+mn-lt");
    assert!(h.query_by_label("Text style: Body 3 (copy)").is_some(), "a title box can't follow a body style: it's a copy");
}

#[test]
fn nothing_selected_lists_text_styles_and_edits_one_for_the_deck() {
    let mut h = harness();
    let app = h.state_mut();
    let title = app.session.active().unwrap().current_slide().unwrap().shapes[0].id.0;
    app.session.execute("text.set", &json!({"id": title, "text": "Pixel Grid"})).unwrap();
    h.run_steps(3);
    for name in ["Title", "Body", "Body 2", "Body 3", "Plain text"] {
        assert!(h.query_by_label(&format!("Edit text style {name}")).is_some(), "{name} is listed");
    }
    h.get_by_label("Edit text style Title").click();
    h.run_steps(3);
    assert!(h.query_by_label("Back to text styles").is_some(), "the style's editor is open");
    h.get_by_label("Italic").click();
    h.run_steps(3);
    let app = h.state_mut();
    app.session.execute("selection.select", &json!({"ids": [title]})).unwrap();
    let st = app.session.execute("format.state", &json!({})).unwrap();
    assert_eq!(st["italic"], true, "the title follows its edited style");
    app.session.execute("edit.deselect", &json!({})).unwrap();
    h.run_steps(3);
    h.get_by_label("Back to text styles").click();
    h.run_steps(3);
    assert!(h.query_by_label("Edit text style Title").is_some(), "back to the list");
}

#[test]
fn slide_master_view_edits_the_master_and_layouts() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.run("view.shell", json!({"shell": "canvas"})).unwrap();
    app.run("view.slideMaster", json!({"master": 0, "layout": null})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1600.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    for label in ["Theme colors", "Theme fonts", "Background", "Theme color accent1"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    for label in ["Apply to all", "Hide background graphics", "Layout"] {
        assert!(h.query_by_label(label).is_none(), "the master has no {label}");
    }
    // A theme colour typed as hex.
    h.state_mut().run("design.colors", json!({"colors": {"accent1": "#123456"}})).unwrap();
    let m = |h: &Harness<'static, SlideApp>| h.state().session.active().unwrap().doc.masters[0].clone();
    assert_eq!(m(&h).theme.colors.get(deckcraft_color::SchemeSlot::Accent1).hex(), "123456");
    // A layout: its own background and hiding the master's graphics.
    h.state_mut().run("view.slideMaster", json!({"master": 0, "layout": 1})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Hide background graphics").is_some());
    assert!(h.query_by_label("Apply to all").is_none(), "slides-only");
    h.get_by_label("Hide background graphics").click();
    h.run_steps(3);
    assert!(!m(&h).layouts[1].show_master_shapes);
    h.state_mut().run("design.background", json!({"color": "#FF0000"})).unwrap();
    assert!(m(&h).layouts[1].background.is_some());
    assert!(m(&h).background != m(&h).layouts[1].background, "the master keeps its own");
}

#[test]
fn the_slide_section_resets_the_slide() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.run("view.shell", json!({"shell": "canvas"})).unwrap();
    let title = app.session.active().unwrap().current_slide().unwrap().shapes[0].id.0;
    app.run("selection.select", json!({"ids": [title]})).unwrap();
    app.run("shape.move", json!({"x": 40})).unwrap();
    app.run("slide.go", json!({"index": 0})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1400.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.get_by_label("Reset slide to its layout").click();
    h.run_steps(3);
    let st = h.state().session.active().unwrap();
    assert!(st.current_slide().unwrap().shapes.iter().find(|s| s.id.0 == title).is_none_or(|s| s.xfrm.is_none()), "the title follows its layout");
}
