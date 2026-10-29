//! Measurement units (#1326): with centimetres chosen in Preferences, the Format pane, the ribbon
//! and the Slide Size dialog show and take centimetres, and the deck keeps its sizes in points.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};
use deckcraft_engine::Session;
use deckcraft_geom::Unit;

fn harness(app: SlideApp) -> Harness<'static, SlideApp> {
    Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    )
}

/// A blank slide with one 2" x 1.5" rectangle at (1", 0.5"), selected.
fn app_with_shape(units: Unit) -> SlideApp {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [72.0, 36.0, 144.0, 108.0]})).unwrap()["id"].as_u64().unwrap();
    s.execute("edit.select", &json!({"ids": [id]})).unwrap();
    s.prefs.units = units;
    SlideApp::new(s, Services::default())
}

fn text_values(h: &Harness<'_, SlideApp>) -> Vec<String> {
    h.query_all_by_role(Role::TextInput).filter_map(|n| n.value()).collect()
}

fn selected_xfrm(app: &SlideApp) -> deckcraft_geom::Xfrm {
    let st = app.session.active().unwrap();
    let sh = st.shape(*st.selection.shapes.first().unwrap()).unwrap();
    deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh)
}

#[test]
fn the_format_pane_shows_and_takes_centimetres() {
    let mut app = app_with_shape(Unit::Centimeters);
    app.ui.pane = Some("format".into());
    app.ui.format_tab = "size".into();
    app.ui.ruler = true;
    app.ui.tab = "Shape Format".into();
    let mut h = harness(app);
    h.run_steps(3);
    h.state_mut().ui.tab = "Home".into();
    h.run_steps(3);
    let shown = text_values(&h);
    // 108 pt = 1.5" = 3.81 cm; 144 pt = 5.08 cm; 72 pt = 2.54 cm; 36 pt = 1.27 cm.
    for v in ["3.81 cm", "5.08 cm", "2.54 cm", "1.27 cm"] {
        assert!(shown.iter().any(|s| s == v), "{v} in {shown:?}");
    }
    assert!(shown.iter().all(|s| !s.ends_with('"')), "no inches left: {shown:?}");
    // Typing 2.54 into Width makes the shape one inch wide, not 2.54 inches.
    let width = h.query_all_by_role(Role::TextInput).find(|n| n.value().as_deref() == Some("5.08 cm")).unwrap();
    width.focus();
    h.run_steps(1);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.query_all_by_role(Role::TextInput).find(|n| n.is_focused()).unwrap().type_text("2.54");
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    let x = selected_xfrm(h.state());
    assert!((x.w - 72.0).abs() < 1e-6, "{x:?}");
    assert!((x.h - 108.0).abs() < 1e-6, "the height is untouched: {x:?}");
}

#[test]
fn the_format_pane_keeps_inches_by_default() {
    let mut app = app_with_shape(Unit::default());
    app.ui.pane = Some("format".into());
    app.ui.format_tab = "size".into();
    let mut h = harness(app);
    h.run_steps(3);
    let shown = text_values(&h);
    assert!(shown.iter().any(|s| s == "2\""), "{shown:?}");
    assert!(shown.iter().all(|s| !s.ends_with("cm")), "{shown:?}");
}

/// Shape Format › Size on the ribbon: the same unit as the pane (the pane stays closed here).
#[test]
fn the_ribbon_size_boxes_show_and_take_centimetres() {
    let mut app = app_with_shape(Unit::Centimeters);
    app.ui.tab = "Shape Format".into();
    let mut h = harness(app);
    h.run_steps(3);
    let shown = text_values(&h);
    assert!(shown.iter().any(|s| s == "3.81 cm") && shown.iter().any(|s| s == "5.08 cm"), "{shown:?}");
    let height = h.query_all_by_role(Role::TextInput).find(|n| n.value().as_deref() == Some("3.81 cm")).unwrap();
    height.focus();
    h.run_steps(1);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.query_all_by_role(Role::TextInput).find(|n| n.is_focused()).unwrap().type_text("1.27");
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    let x = selected_xfrm(h.state());
    assert!((x.h - 36.0).abs() < 1e-6 && (x.w - 144.0).abs() < 1e-6, "{x:?}");
}

#[test]
fn the_slide_size_dialog_shows_and_takes_centimetres() {
    let mut app = app_with_shape(Unit::Centimeters);
    app.dialog = Some(crate::dialogs::Dialog::new("slideSize"));
    let mut h = harness(app);
    h.run_steps(3);
    assert!(h.query_by_label("Width (cm):").is_some() && h.query_by_label("Height (cm):").is_some());
    assert!(h.query_by_label("Width (in):").is_none());
    // 960 x 540 pt = 33.87 x 19.05 cm.
    let shown = text_values(&h);
    assert!(shown.iter().any(|s| s == "33.87") && shown.iter().any(|s| s == "19.05"), "{shown:?}");
    // A 4:3 size typed in centimetres: 25.4 x 19.05 cm = 10" x 7.5" = 720 x 540 pt.
    let d = h.state_mut().dialog.as_mut().unwrap();
    d.fields.insert("w".into(), "25.4".into());
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(3);
    let size = h.state().session.active().unwrap().doc.slide_size;
    assert!((size.width - 720.0).abs() < 1e-6 && (size.height - 540.0).abs() < 1e-6, "{size:?}");
}

/// OK with the shown values untouched keeps the exact size: 13.333" is shown rounded to 33.87 cm,
/// which read back would make the slide 960.09 pt wide.
#[test]
fn ok_in_the_slide_size_dialog_without_edits_keeps_the_size() {
    for units in [Unit::Centimeters, Unit::Inches] {
        let mut app = app_with_shape(units);
        app.dialog = Some(crate::dialogs::Dialog::new("slideSize"));
        let mut h = harness(app);
        h.run_steps(3);
        h.get_by_label("OK").click();
        h.run_steps(3);
        let size = h.state().session.active().unwrap().doc.slide_size;
        assert_eq!((size.width, size.height), (960.0, 540.0), "{units:?}");
    }
}

fn slide_size_dialog(units: Unit, w: f64, h: f64) -> Harness<'static, SlideApp> {
    let mut app = app_with_shape(units);
    app.session.execute("design.slideSize", &json!({"w": w, "h": h, "scale": "none"})).unwrap();
    app.dialog = Some(crate::dialogs::Dialog::new("slideSize"));
    let mut h = harness(app);
    h.run_steps(3);
    h
}

fn ok_and_size(h: &mut Harness<'_, SlideApp>) -> (f64, f64) {
    h.get_by_label("OK").click();
    h.run_steps(3);
    let size = h.state().session.active().unwrap().doc.slide_size;
    (size.width, size.height)
}

/// A preset is the preset's exact size, not its two-decimal rendering read back (33.87 cm would
/// be 960.09 pt; in inches 16:9, A4, B4 and B5 drift the same way).
#[test]
fn a_slide_size_preset_gives_its_exact_size() {
    for (units, label, size) in [
        (Unit::Centimeters, "Widescreen (16:9)", (960.0, 540.0)),
        (Unit::Inches, "Widescreen (16:9)", (960.0, 540.0)),
        (Unit::Inches, "A4 Paper (210x297 mm)", (780.0, 540.0)),
    ] {
        let mut h = slide_size_dialog(units, 720.0, 540.0);
        h.get_by_role_and_label(Role::ComboBox, "Slides sized for:").click();
        h.run_steps(2);
        h.get_by_label(label).click();
        h.run_steps(2);
        assert_eq!(ok_and_size(&mut h), size, "{units:?} {label}");
    }
}

/// Portrait swaps the exact width and height.
#[test]
fn portrait_swaps_the_exact_slide_size() {
    for units in [Unit::Centimeters, Unit::Inches] {
        let mut h = slide_size_dialog(units, 960.0, 540.0);
        h.get_by_label("Portrait").click();
        h.run_steps(2);
        assert_eq!(ok_and_size(&mut h), (540.0, 960.0), "{units:?}");
    }
}

#[test]
fn preferences_choose_the_units_and_cancel_puts_them_back() {
    let mut app = app_with_shape(Unit::Inches);
    app.dialog = Some(crate::dialogs::Dialog::new("preferences"));
    let mut h = harness(app);
    h.run_steps(3);
    h.get_by_label("Inches").click();
    h.run_steps(2);
    h.get_by_label("Centimeters").click();
    h.run_steps(2);
    assert_eq!(h.state().session.prefs.units, Unit::Centimeters);
    h.query_all_by_role_and_label(Role::Button, "Cancel").next().unwrap().click_accesskit();
    h.run_steps(2);
    assert_eq!(h.state().session.prefs.units, Unit::Inches);
}

#[test]
fn the_ruler_counts_centimetres_in_halves_and_inches_in_quarters() {
    assert_eq!(crate::canvas::ruler_steps(Unit::Inches), (72.0, 4));
    let (cm, sub) = crate::canvas::ruler_steps(Unit::Centimeters);
    assert!((cm - 72.0 / 2.54).abs() < 1e-9 && sub == 2, "{cm} {sub}");
    // A hand-edited prefs.json can say points: the ruler stays in inches rather than a tick per point.
    assert_eq!(crate::canvas::ruler_steps(Unit::Points), (72.0, 4));
}
