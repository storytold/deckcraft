//! The property editor: its sections, and that its controls run the commands they name.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use deckcraft_model::{Fill, ShapeId};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use super::*;
use crate::{Services, SlideApp};

fn harness() -> Harness<'static, SlideApp> {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.run("view.shell", json!({"shell": "canvas"})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1600.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

/// A blue rectangle with text, selected.
fn select_box(h: &mut Harness<'static, SlideApp>, preset: &str) -> u32 {
    let app = h.state_mut();
    let id = app.session.execute("shape.insert", &json!({"preset": preset, "rect": [100, 100, 200, 100], "text": "Hi"})).unwrap()["id"]
        .as_u64()
        .unwrap() as u32;
    app.session.execute("selection.select", &json!({"ids": [id]})).unwrap();
    app.session.execute("shape.fill", &json!({"color": "#3366CC"})).unwrap();
    h.run_steps(3);
    id
}

fn shape(app: &SlideApp, id: u32) -> Shape {
    app.session.active().unwrap().shape(ShapeId(id)).cloned().unwrap()
}

fn click(h: &mut Harness<'static, SlideApp>, label: &str) {
    h.get_by_label(label).click();
    h.run_steps(3);
}

#[test]
fn a_shape_shows_every_section() {
    let mut h = harness();
    select_box(&mut h, "rect");
    for label in [
        "Rectangle",
        "Position",
        "Layout",
        "Appearance",
        "Typography",
        "Fill",
        "Stroke",
        "Effects",
        "Export",
        "Link",
        "Align left",
        "Constrain proportions",
    ] {
        assert!(h.query_all_by_label(label).next().is_some(), "`{label}` is shown");
    }
    // Numbers in pixels, without a unit: x 100 pt is 200, the fill is 3366CC at 100%.
    for value in ["200", "400", "3366CC", "100%"] {
        assert!(h.query_all_by_value(value).next().is_some(), "a field reads {value}");
    }
}

#[test]
fn position_buttons_align_rotate_and_flip() {
    let mut h = harness();
    let id = select_box(&mut h, "rect");
    click(&mut h, "Align left");
    assert_eq!(shape(h.state(), id).xfrm.unwrap().x, 0.0);
    click(&mut h, "Flip horizontal");
    assert!(shape(h.state(), id).xfrm.unwrap().flip_h);
    click(&mut h, "Rotate 90° right");
    assert_eq!(shape(h.state(), id).xfrm.unwrap().rot, 90.0);
}

#[test]
fn resizing_buttons_set_wrap_and_autofit() {
    let mut h = harness();
    let id = select_box(&mut h, "rect");
    click(&mut h, "Auto width");
    assert_eq!(resizing_of(&shape(h.state(), id)), Resizing::AutoWidth);
    click(&mut h, "Auto height");
    assert_eq!(resizing_of(&shape(h.state(), id)), Resizing::AutoHeight);
    click(&mut h, "Fixed size");
    assert_eq!(resizing_of(&shape(h.state(), id)), Resizing::Fixed);
}

#[test]
fn stroke_and_effects_are_added_and_removed() {
    let mut h = harness();
    let id = select_box(&mut h, "rect");
    click(&mut h, "Remove stroke");
    assert!(matches!(shape(h.state(), id).line.and_then(|l| l.fill), Some(Fill::None)));
    click(&mut h, "Add stroke");
    assert!(matches!(shape(h.state(), id).line.and_then(|l| l.fill), Some(Fill::Solid { .. })));
    click(&mut h, "Add effect");
    click(&mut h, "Drop shadow");
    assert!(shape(h.state(), id).effects.and_then(|e| e.outer_shadow).is_some());
    click(&mut h, "Remove drop shadow");
    assert!(shape(h.state(), id).effects.and_then(|e| e.outer_shadow).is_none());
}

#[test]
fn a_text_box_fill_is_its_text_colour() {
    let mut h = harness();
    let app = h.state_mut();
    let id = app.session.execute("insert.textBox", &json!({"rect": [100, 100, 300, 80], "text": "Words"})).unwrap()["id"].as_u64().unwrap();
    app.session.execute("selection.select", &json!({"ids": [id]})).unwrap();
    app.run("format.color", json!({"color": "#AA2200"})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Text").is_some(), "the header names a text layer");
    assert!(h.query_all_by_value("AA2200").next().is_some(), "Fill shows the text colour");
}

#[test]
fn type_details_open_from_the_tune_button() {
    let mut h = harness();
    select_box(&mut h, "rect");
    assert!(h.query_by_label("Bold (⌘B)").is_none(), "details start folded");
    click(&mut h, "Type details: bold, italic, underline, lists");
    click(&mut h, "Bold (⌘B)");
    let st = h.state_mut().session.execute("format.state", &json!({})).unwrap();
    assert_eq!(st["bold"], true);
}

#[test]
fn model_helpers() {
    assert_eq!(parse_hex("fff").as_deref(), Some("#FFFFFF"));
    assert_eq!(parse_hex("#3366cc").as_deref(), Some("#3366CC"));
    assert_eq!(parse_hex("33x"), None);
    assert_eq!(parse_hex("12345"), None);
    let mut s = Session::with_new();
    let id = s.execute("shape.insert", &json!({"preset": "roundRect", "rect": [0, 0, 200, 100]})).unwrap()["id"].as_u64().unwrap();
    let st = s.doc().unwrap();
    let sh = st.shape(ShapeId(id as u32)).unwrap().clone();
    let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, &sh);
    let r = corner_radius(&sh, &x).unwrap();
    assert!((r - 16.667).abs() < 0.01, "{r}: 16.667% of the short side");
    assert_eq!(kind_label(&sh), "Rectangle: Rounded Corners", "the shape catalog's name");
    let sc = deckcraft_model::Theme::default().colors;
    assert_eq!(opacity_of(&sh, &sc), 1.0);
}

fn three_selected(h: &mut Harness<'static, SlideApp>) -> Vec<u32> {
    let app = h.state_mut();
    let ids: Vec<u32> = [[100, 100, 100, 50], [260, 140, 80, 50], [400, 120, 120, 60]]
        .iter()
        .map(|r| app.session.execute("shape.insert", &json!({"preset": "rect", "rect": r})).unwrap()["id"].as_u64().unwrap() as u32)
        .collect();
    app.session.execute("selection.select", &json!({"ids": ids})).unwrap();
    h.run_steps(3);
    ids
}

#[test]
fn several_layers_show_a_count_alignment_and_spacing() {
    let mut h = harness();
    let ids = three_selected(&mut h);
    for label in ["3 selected", "Align left", "Distribute", "Spacing", "Group (⌘G)", "Merge shapes"] {
        assert!(h.query_by_label(label).is_some(), "`{label}` is shown");
    }
    click(&mut h, "Align top");
    let ys: Vec<f64> = ids.iter().map(|id| shape(h.state(), *id).xfrm.unwrap().y).collect();
    assert!(ys.iter().all(|y| (y - 100.0).abs() < 1e-6), "aligned to the top one: {ys:?}");
    click(&mut h, "Group (⌘G)");
    let d = h.state().session.active().unwrap();
    assert!(d.selected_shapes().iter().any(|s| matches!(s.kind, ShapeKind::Group { .. })), "grouped");
}

#[test]
fn spacing_is_the_gap_between_neighbours_and_sets_them_equal() {
    let b = |id: u32, x: f64, w: f64| (ShapeId(id), deckcraft_model::Xfrm { x, y: 0.0, w, h: 10.0, ..Default::default() });
    // Out of order on purpose: neighbours go left to right.
    let boxes = [b(1, 300.0, 50.0), b(2, 0.0, 100.0), b(3, 130.0, 70.0)];
    assert_eq!(gaps(&boxes, true), vec![30.0, 100.0]);
    assert_eq!(common(gaps(&boxes, true)), None, "unequal gaps are Mixed");
    assert_eq!(spaced(&boxes, true, 20.0), vec![(ShapeId(3), 120.0), (ShapeId(1), 210.0)]);
    assert_eq!(common([20.0, 20.001]), Some(20.0));
    assert_eq!(common([20.0, 20.5]), None, "a typed half pixel counts");
}

fn run_colors(app: &SlideApp, id: u32) -> Vec<Option<Fill>> {
    shape(app, id).text.unwrap().paragraphs.iter().flat_map(|p| p.runs.iter().map(|r| r.props.fill.clone())).collect()
}

fn red() -> Option<Fill> {
    Some(Fill::solid(deckcraft_model::ColorRef::rgb(deckcraft_color::Rgba::rgb(255, 0, 0))))
}

#[test]
fn text_colour_reaches_the_whole_box_while_typing_with_a_bare_caret() {
    let mut h = harness();
    let app = h.state_mut();
    let id = app.run("insert.textBox", json!({"rect": [50, 50, 300, 40], "text": "Hello world"})).unwrap()["id"].as_u64().unwrap() as u32;
    app.run("text.edit", json!({"id": id, "end": true})).unwrap();
    crate::textstyles::run_on_text(app, "Font Color", vec![("format.color", json!({"color": "#FF0000"}))]);
    assert!(run_colors(app, id).iter().all(|f| *f == red()), "every run: {:?}", run_colors(app, id));
    let t = app.session.active().unwrap().selection.text.clone().expect("still typing");
    assert_eq!((t.anchor, t.caret), ((0, 11), (0, 11)), "the caret stays at the end");
}

#[test]
fn a_text_style_keeps_the_texts_own_colour() {
    let mut h = harness();
    let app = h.state_mut();
    let id = app.run("insert.textBox", json!({"rect": [50, 50, 300, 40], "text": "Hello"})).unwrap()["id"].as_u64().unwrap() as u32;
    app.run("format.color", json!({"color": "#FF0000"})).unwrap();
    let doc = app.session.active().unwrap().doc.clone();
    let styles = crate::textstyles::styles(&doc);
    // Plain text links (the box inherits it); Title is copied.
    for name in ["Plain text", "Title"] {
        let style = styles.iter().find(|s| s.name == name).unwrap();
        crate::textstyles::apply(app, style);
        assert!(run_colors(app, id).iter().all(|f| *f == red()), "{name}: {:?}", run_colors(app, id));
    }
}

#[test]
fn a_shape_with_text_has_a_text_colour_row() {
    let mut h = harness();
    select_box(&mut h, "rect");
    assert!(h.query_by_label("Text color").is_some(), "its Fill is the shape's, so the text gets its own colour swatch");
    let mut h = harness();
    let app = h.state_mut();
    app.run("insert.textBox", json!({"rect": [50, 50, 300, 40], "text": "Hello"})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Text color").is_none(), "a text box's Fill is its text colour");
    assert!(h.query_by_label("Fill color").is_some());
}

fn text_box(h: &mut Harness<'static, SlideApp>) -> u32 {
    let id = h.state_mut().run("insert.textBox", json!({"rect": [50, 50, 300, 40], "text": "Hello"})).unwrap()["id"].as_u64().unwrap() as u32;
    h.run_steps(3);
    id
}

#[test]
fn styled_text_shows_its_style_and_unstyled_text_its_type_controls() {
    let mut h = harness();
    text_box(&mut h);
    assert!(h.query_by_label("Text style: Plain text").is_some(), "follows the presentation's default text style");
    assert!(h.query_by_label("Text styles").is_none(), "the chip itself opens the styles");
    assert!(h.query_by_label("Weight: Regular").is_none(), "type controls are folded into the style");
    // Its own size: no longer the style.
    h.state_mut().run("format.size", json!({"size": 31})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Weight: Regular").is_some());
    assert!(h.query_by_label("Text styles").is_some(), "a button applies a style");
}

#[test]
fn detaching_keeps_the_look_and_shows_the_type_controls() {
    let mut h = harness();
    let id = text_box(&mut h);
    let before = h.state_mut().session.execute("format.state", &json!({})).unwrap()["size"].clone();
    click(&mut h, "Detach style");
    assert!(h.query_by_label("Weight: Regular").is_some(), "every type control");
    assert!(h.query_by_label("Text style: Plain text").is_none());
    let sh = shape(h.state(), id);
    assert!(sh.text.unwrap().paragraphs[0].runs[0].props.size.is_some(), "the style's values are the text's own");
    assert_eq!(h.state_mut().session.execute("format.state", &json!({})).unwrap()["size"], before, "same size");
    // Applying a style again follows it.
    let doc = h.state().session.active().unwrap().doc.clone();
    let plain = crate::textstyles::styles(&doc).into_iter().find(|s| s.name == "Plain text").unwrap();
    crate::textstyles::apply(h.state_mut(), &plain);
    h.run_steps(3);
    assert!(h.query_by_label("Text style: Plain text").is_some());
}

#[test]
fn padding_sets_the_text_box_insets() {
    let mut h = harness();
    let id = text_box(&mut h);
    let app = h.state();
    let sh = shape(app, id);
    assert_eq!(insets(app, &sh), [7.2, 3.6, 7.2, 3.6], "PowerPoint's default insets");
    // Both sides at once (10 px = 5 pt), then each side.
    h.state_mut().run("format.margins", json!({"left": 5, "right": 5})).unwrap();
    h.run_steps(3);
    let sh = shape(h.state(), id);
    assert_eq!(insets(h.state(), &sh)[0], 5.0);
    assert!(h.query_by_label("Padding for each side").is_some());
    click(&mut h, "Padding for each side");
    assert!(h.query_by_label("Horizontal and vertical padding").is_some(), "each side shown");
}

fn arrow(h: &mut Harness<'static, SlideApp>, key: egui::Key, modifiers: egui::Modifiers) {
    h.event(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
    h.event(egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers });
    h.run_steps(3);
}

/// The first field (top of the panel down) showing `value`.
fn focus_field(h: &mut Harness<'static, SlideApp>, value: &str) {
    h.query_all_by_value(value).next().expect("a field reads the value").click();
    h.run_steps(3);
}

fn rect_selected(h: &mut Harness<'static, SlideApp>, preset: &str) -> u32 {
    let app = h.state_mut();
    // 150 × 60 pt: W reads 300 px, H 120.
    let id = app.run("shape.insert", json!({"preset": preset, "rect": [100, 100, 150, 60]})).unwrap()["id"].as_u64().unwrap() as u32;
    app.run("selection.select", json!({"ids": [id]})).unwrap();
    h.run_steps(3);
    id
}

#[test]
fn arrow_keys_step_a_focused_field_by_one_or_ten() {
    let mut h = harness();
    let id = rect_selected(&mut h, "rect");
    focus_field(&mut h, "300");
    arrow(&mut h, egui::Key::ArrowUp, egui::Modifiers::NONE);
    assert_eq!(shape(h.state(), id).xfrm.unwrap().w, 150.5, "301 px");
    arrow(&mut h, egui::Key::ArrowUp, egui::Modifiers::SHIFT);
    assert_eq!(shape(h.state(), id).xfrm.unwrap().w, 155.5, "311 px");
    arrow(&mut h, egui::Key::ArrowDown, egui::Modifiers::NONE);
    assert_eq!(shape(h.state(), id).xfrm.unwrap().w, 155.0, "310 px");
    assert_eq!(shape(h.state(), id).xfrm.unwrap().x, 100.0, "the arrows didn't nudge the shape");
}

#[test]
fn fields_keep_to_their_range() {
    let mut h = harness();
    select_box(&mut h, "rect");
    focus_field(&mut h, "100%");
    arrow(&mut h, egui::Key::ArrowUp, egui::Modifiers::SHIFT);
    let sc = deckcraft_model::Theme::default().colors;
    let d = h.state().session.active().unwrap();
    assert_eq!(opacity_of(d.selected_shapes()[0], &sc), 1.0, "opacity stops at 100%");
}

#[test]
fn a_picture_keeps_its_proportions_by_default() {
    let mut h = harness();
    let png = deckcraft_render::Image { width: 4, height: 2, pixels: vec![200; 32] }.to_png();
    let id = h
        .state_mut()
        .run("insert.picture", json!({"name": "p.png", "data": deckcraft_engine::cmd::base64_encode(&png), "rect": [100, 100, 150, 75]}))
        .unwrap()["id"]
        .as_u64()
        .unwrap() as u32;
    h.run_steps(3);
    let x = shape(h.state(), id).xfrm.unwrap();
    let w_px = format!("{}", (x.w * 2.0).round());
    focus_field(&mut h, &w_px);
    arrow(&mut h, egui::Key::ArrowUp, egui::Modifiers::SHIFT);
    let y = shape(h.state(), id).xfrm.unwrap();
    assert!((y.w / y.h - x.w / x.h).abs() < 1e-6, "the height followed: {x:?} → {y:?}");
}

#[test]
fn corner_radii_round_trip_through_the_presets_pptx_has() {
    let mut s = Session::with_new();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [0, 0, 200, 100]})).unwrap()["id"].as_u64().unwrap();
    let get = |s: &Session| {
        let st = s.doc().unwrap();
        let sh = st.shape(ShapeId(id as u32)).unwrap().clone();
        let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, &sh);
        corners(&sh, &x).unwrap().map(|v| (v * 100.0).round() / 100.0)
    };
    assert_eq!(get(&s), [0.0; 4], "a rectangle has square corners");
    for want in [[10.0; 4], [10.0, 10.0, 4.0, 4.0], [10.0, 4.0, 10.0, 4.0], [0.0, 12.0, 0.0, 0.0], [0.0; 4]] {
        let now = match &s.doc().unwrap().shape(ShapeId(id as u32)).unwrap().geom {
            Geom::Preset { name, .. } => name.clone(),
            Geom::Custom { .. } => String::new(),
        };
        for (cmd, p) in corner_commands(ShapeId(id as u32), &now, want, 100.0).unwrap() {
            s.execute(cmd, &p).unwrap();
        }
        assert_eq!(get(&s), want);
    }
    assert!(corner_preset([1.0, 2.0, 3.0, 4.0], 100.0).is_none(), "four different corners don't exist in PPTX");
    assert_eq!(corner_preset([60.0; 4], 100.0).unwrap().1, vec![50_000.0], "at most half the shorter side");
}

#[test]
fn a_rectangle_gets_a_corner_radius_and_each_corner_on_its_own() {
    let mut h = harness();
    rect_selected(&mut h, "rect");
    assert!(h.query_by_label("Each corner on its own").is_some());
    click(&mut h, "Each corner on its own");
    assert!(h.query_by_label("One radius for every corner").is_some(), "four fields shown");
    let mut h = harness();
    text_box(&mut h);
    assert!(h.query_by_label("Each corner on its own").is_none(), "a text box has no corners to round");
}

#[test]
fn updating_a_style_from_the_selection_makes_the_text_follow_it() {
    let mut h = harness();
    let id = text_box(&mut h);
    h.state_mut().run("format.size", json!({"size": 31})).unwrap();
    h.run_steps(3);
    let undo = h.state().session.active().unwrap().history.undo.len();
    let doc = h.state().session.active().unwrap().doc.clone();
    let plain = crate::textstyles::styles(&doc).into_iter().find(|s| s.name == "Plain text").unwrap();
    crate::textstyles::update_from_selection(h.state_mut(), &plain);
    h.run_steps(3);
    let doc = h.state().session.active().unwrap().doc.clone();
    let plain = crate::textstyles::styles(&doc).into_iter().find(|s| s.name == "Plain text").unwrap();
    assert_eq!(plain.size(), 31.0, "the style took the text's size");
    assert!(shape(h.state(), id).text.unwrap().paragraphs[0].runs[0].props.size.is_none(), "the text follows the style");
    assert!(h.query_by_label("Text style: Plain text").is_some());
    assert_eq!(h.state().session.active().unwrap().history.undo.len(), undo + 1, "one undo step");
}

#[test]
fn a_slides_placeholder_is_named_by_its_kind_without_a_tag_menu() {
    let mut h = harness();
    let title = h.state().session.active().unwrap().current_slide().unwrap().shapes[0].id.0;
    h.state_mut().run("selection.select", json!({"ids": [title]})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Title placeholder").is_none());
    assert!(h.query_by_label("Placeholder: what this box is on the slide's layout").is_none(), "tags are set in Slide Master view");
}

#[test]
fn a_box_on_a_layout_is_tagged_from_the_same_menu() {
    let mut h = harness();
    h.state_mut().run("view.slideMaster", json!({"master": 0, "layout": 1})).unwrap();
    h.run_steps(2);
    let id = text_box(&mut h);
    click(&mut h, "Placeholder: what this box is on the slide's layout");
    assert!(h.query_by_label("Make this a placeholder").is_some());
    assert!(h.query_by_label("Picture").is_some(), "a layout takes kinds its slides don't have yet");
    click(&mut h, "Footer");
    assert_eq!(shape(h.state(), id).ph_type(), Some(deckcraft_model::PhType::Footer));
    h.get_by_label("Footer placeholder");
}

#[test]
fn a_moved_placeholder_resets_from_the_header() {
    let mut h = harness();
    let title = h.state().session.active().unwrap().current_slide().unwrap().shapes[0].id.0;
    h.state_mut().run("selection.select", json!({"ids": [title]})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Reset to layout").is_none(), "nothing to reset yet");
    h.state_mut().run("shape.move", json!({"x": 40})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Apply to all").is_none(), "nothing that changes other slides");
    click(&mut h, "Reset to layout");
    assert!(shape(h.state(), title).xfrm.is_none(), "it follows its layout again");
}
