//! The Columns dialog: Home › Columns › More Columns… and Format Shape › Text Box › Columns… open it
//! with the text box's values, and OK sets the number and spacing in one undo step.

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

fn harness() -> Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, 80.0, 320.0, 160.0], "text": "Hello"})).unwrap();
    let _ = s.execute("text.exit", &json!({}));
    harness_of(s)
}

/// Two text boxes with different columns, both selected: 2 columns 9 pt apart, and 3 columns
/// 36 pt apart.
fn two_boxes() -> Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    let mut ids = Vec::new();
    for (y, count, spacing) in [(80.0, 2, 9.0), (300.0, 3, 36.0)] {
        ids.push(s.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, y, 320.0, 160.0], "text": "Hello"})).unwrap()["id"].clone());
        let _ = s.execute("text.exit", &json!({}));
        s.execute("format.columns", &json!({"count": count, "spacing": spacing})).unwrap();
    }
    s.execute("edit.select", &json!({"ids": ids})).unwrap();
    harness_of(s)
}

fn harness_of(s: Session) -> Harness<'static, SlideApp> {
    let app = SlideApp::new(s, Services::default());
    let mut h = Harness::builder().with_size(egui::vec2(1440.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

fn columns_of(h: &Harness<'_, SlideApp>) -> (Option<u32>, Option<f64>) {
    let st = h.state().session.active().unwrap();
    let body = st.selected_shapes().first().and_then(|sh| sh.text.as_ref()).map(|t| t.body.clone()).unwrap_or_default();
    (body.columns, body.col_spacing)
}

fn columns_of_each(h: &Harness<'_, SlideApp>) -> Vec<(Option<u32>, Option<f64>)> {
    let st = h.state().session.active().unwrap();
    st.selected_shapes().iter().map(|sh| sh.text.as_ref().map(|t| (t.body.columns, t.body.col_spacing)).unwrap_or_default()).collect()
}

/// Opens the dialog on `h`'s selection, types `fields` into it and presses OK.
fn ok_with(h: &mut Harness<'_, SlideApp>, fields: &[(&str, &str)]) {
    h.state_mut().run("format.columns", json!({})).unwrap();
    h.run_steps(2);
    let d = h.state_mut().dialog.as_mut().unwrap();
    for (key, value) in fields {
        d.fields.insert((*key).into(), (*value).into());
    }
    h.get_by_label("OK").click_accesskit();
    h.run_steps(2);
    assert!(h.state().dialog.is_none(), "status {:?}", h.state().status);
}

/// With text boxes whose columns differ, the dialog shows the first one's values, and a field
/// left as shown does not overwrite the others' own values.
#[test]
fn a_field_left_as_shown_keeps_each_text_box_s_own_value() {
    let mut h = two_boxes();
    assert_eq!(columns_of_each(&h), [(Some(2), Some(9.0)), (Some(3), Some(36.0))]);
    ok_with(&mut h, &[("count", "4")]);
    assert_eq!(columns_of_each(&h), [(Some(4), Some(9.0)), (Some(4), Some(36.0))], "Number only");

    let mut h = two_boxes();
    ok_with(&mut h, &[("spacing", "0.5")]);
    assert_eq!(columns_of_each(&h), [(Some(2), Some(36.0)), (Some(3), Some(36.0))], "Spacing only");
}

#[test]
fn ok_with_nothing_changed_closes_without_an_edit() {
    let mut h = two_boxes();
    let n = h.state().session.active().unwrap().history.undo.len();
    ok_with(&mut h, &[]);
    assert_eq!(h.state().session.active().unwrap().history.undo.len(), n);
    assert_eq!(columns_of_each(&h), [(Some(2), Some(9.0)), (Some(3), Some(36.0))]);
}

fn field(h: &Harness<'_, SlideApp>, key: &str) -> Option<String> {
    h.state().dialog.as_ref().and_then(|d| d.fields.get(key).cloned())
}

#[test]
fn more_columns_sets_the_number_and_spacing_in_one_undo() {
    let mut h = harness();
    h.state_mut().session.execute("format.columns", &json!({"count": 3, "spacing": 9.0})).unwrap();
    h.run_steps(2);
    h.get_by_label("Columns").click();
    h.run_steps(2);
    h.get_by_label("More Columns…").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("columns"), "status {:?}", h.state().status);
    assert_eq!(field(&h, "count").as_deref(), Some("3"));
    assert_eq!(field(&h, "spacing").as_deref(), Some("0.12"));
    {
        let d = h.state_mut().dialog.as_mut().unwrap();
        d.fields.insert("count".into(), "2".into());
        d.fields.insert("spacing".into(), "0.5".into());
    }
    let n = h.state().session.active().unwrap().history.undo.len();
    h.get_by_label("OK").click_accesskit();
    h.run_steps(2);
    assert!(h.state().dialog.is_none(), "status {:?}", h.state().status);
    assert_eq!(columns_of(&h), (Some(2), Some(36.0)));
    let undo = &h.state().session.active().unwrap().history.undo;
    assert_eq!(undo.len(), n + 1);
    assert_eq!(undo.last().unwrap().label, "Columns");
}

#[test]
fn ok_with_the_spacing_unchanged_keeps_the_exact_spacing() {
    let mut h = harness();
    h.state_mut().session.execute("format.columns", &json!({"count": 3, "spacing": 9.0})).unwrap();
    h.state_mut().run("format.columns", json!({})).unwrap();
    h.run_steps(2);
    assert_eq!(field(&h, "spacing").as_deref(), Some("0.12"), "shown rounded to two places");
    h.state_mut().dialog.as_mut().unwrap().fields.insert("count".into(), "4".into());
    h.get_by_label("OK").click_accesskit();
    h.run_steps(2);
    assert!(h.state().dialog.is_none(), "status {:?}", h.state().status);
    assert_eq!(columns_of(&h), (Some(4), Some(9.0)));
}

#[test]
fn format_shape_columns_button_opens_the_dialog() {
    let mut h = harness();
    h.state_mut().ui.pane = Some("format".into());
    h.state_mut().ui.format_tab = "text".into();
    h.run_steps(2);
    h.get_by_label("Columns…").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("columns"), "status {:?}", h.state().status);
    assert_eq!(field(&h, "count").as_deref(), Some("1"));
    assert_eq!(field(&h, "spacing").as_deref(), Some("0"));
}

#[test]
fn a_bad_number_keeps_the_dialog_open_and_changes_nothing() {
    let mut h = harness();
    h.state_mut().run("format.columns", json!({})).unwrap();
    h.run_steps(2);
    for (count, spacing) in [("0", "0.25"), ("17", "0.25"), ("two", "0.25"), ("2", "-1"), ("2", "wide")] {
        {
            let d = h.state_mut().dialog.as_mut().unwrap();
            d.fields.insert("count".into(), count.into());
            d.fields.insert("spacing".into(), spacing.into());
        }
        h.get_by_label("OK").click_accesskit();
        h.run_steps(2);
        assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("columns"), "{count} / {spacing}");
        assert!(field(&h, "error").is_some_and(|e| !e.is_empty()), "{count} / {spacing}");
        assert_eq!(columns_of(&h), (None, None), "{count} / {spacing}");
    }
}
