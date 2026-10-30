//! Format Shape ▸ Fill ▸ Gradient fill: the Brightness slider of the selected gradient stop.

use deckcraft_color::{ColorTransform, SchemeSlot};
use deckcraft_engine::Session;
use deckcraft_model::{ColorRef, Fill, ShapeId};
use egui::accesskit::Role;
use egui::{Event, Modifiers, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::panels::fillui::{brightness_of, with_brightness};
use crate::{Services, SlideApp};

/// The values PowerPoint writes for its Brightness slider, read from a saved deck: +40% on accent 1
/// is `lumMod` 60000 with `lumOff` 40000, -40% on red is `lumMod` 60000 alone.
#[test]
fn brightness_is_written_as_powerpoint_writes_it() {
    let accent = ColorRef::scheme(SchemeSlot::Accent1);
    assert_eq!(with_brightness(accent.clone(), 0.4).mods, vec![ColorTransform::LumMod(60_000), ColorTransform::LumOff(40_000)]);
    assert_eq!(with_brightness(accent.clone(), -0.4).mods, vec![ColorTransform::LumMod(60_000)]);
    assert!(with_brightness(accent, 0.0).mods.is_empty());
}

#[test]
fn brightness_reads_back_what_it_wrote_and_keeps_alpha() {
    let accent = ColorRef::scheme(SchemeSlot::Accent1).with(ColorTransform::Alpha(75_000));
    for b in [-1.0, -0.4, 0.0, 0.25, 0.6, 1.0] {
        let c = with_brightness(accent.clone(), b);
        assert!((brightness_of(&c) - b).abs() < 1e-9, "{b}");
        assert!(c.mods.contains(&ColorTransform::Alpha(75_000)), "{b}");
    }
    // Changing brightness replaces the old pair rather than stacking onto it.
    let twice = with_brightness(with_brightness(accent, 0.6), -0.2);
    assert_eq!(twice.mods.iter().filter(|m| matches!(m, ColorTransform::LumMod(_) | ColorTransform::LumOff(_))).count(), 1);
    // Out-of-range input is clamped.
    assert_eq!(brightness_of(&with_brightness(ColorRef::scheme(SchemeSlot::Accent1), 9.0)), 1.0);
}

/// An order-sensitive transform before the pair stays before it, and one after stays after.
#[test]
fn brightness_keeps_the_place_of_the_luminance_transforms() {
    let c = ColorRef::scheme(SchemeSlot::Accent1)
        .with(ColorTransform::Tint(50_000))
        .with(ColorTransform::LumMod(60_000))
        .with(ColorTransform::LumOff(40_000))
        .with(ColorTransform::Alpha(75_000));
    let darker = with_brightness(c, -0.2);
    assert_eq!(darker.mods, vec![ColorTransform::Tint(50_000), ColorTransform::LumMod(80_000), ColorTransform::Alpha(75_000)]);
}

/// A transform between two separated luminance transforms stays between the new pair.
#[test]
fn brightness_keeps_a_transform_between_separated_luminance_transforms() {
    let c = ColorRef::scheme(SchemeSlot::Accent1)
        .with(ColorTransform::LumMod(60_000))
        .with(ColorTransform::Tint(50_000))
        .with(ColorTransform::LumOff(40_000));
    assert_eq!(
        with_brightness(c.clone(), 0.3).mods,
        vec![ColorTransform::LumMod(70_000), ColorTransform::Tint(50_000), ColorTransform::LumOff(30_000)]
    );
    assert_eq!(with_brightness(c, -0.3).mods, vec![ColorTransform::LumMod(70_000), ColorTransform::Tint(50_000)]);
}

/// The order survives the command boundary: `shape.fill` gets the stop as `cref_param` writes it.
#[test]
fn transform_order_survives_the_gradient_command() {
    let c = ColorRef::scheme(SchemeSlot::Accent1)
        .with(ColorTransform::Tint(50_000))
        .with(ColorTransform::LumMod(60_000))
        .with(ColorTransform::LumOff(40_000))
        .with(ColorTransform::Alpha(75_000));
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [40.0, 40.0, 120.0, 80.0]})).unwrap()["id"].as_u64().unwrap();
    s.execute(
        "shape.fill",
        &json!({"id": id, "gradient": {"stops": [[0.0, crate::ribbon::cref_param(&c)], [1.0, "accent2"]], "kind": "linear", "angle": 90}}),
    )
    .unwrap();
    let shape = s.active().unwrap().shape(ShapeId(u32::try_from(id).unwrap())).unwrap();
    let Some(Fill::Gradient(g)) = &shape.fill else { panic!("expected a gradient fill") };
    assert_eq!(g.stops[0].color.mods, c.mods);
    let one = crate::ribbon::cref_param(&ColorRef::scheme(SchemeSlot::Accent1).with(ColorTransform::LumMod(60_000)));
    assert_eq!(one["mods"], json!([{"op": "lumMod", "val": 60_000}]));
    assert!(one.get("lumMod").is_none());
    // A colour without transforms has only its base field.
    assert!(crate::ribbon::cref_param(&ColorRef::scheme(SchemeSlot::Accent1)).get("mods").is_none());
}

fn stop_color(h: &Harness<'_, SlideApp>, id: ShapeId, stop: usize) -> ColorRef {
    match &h.state().session.active().unwrap().shape(id).unwrap().fill {
        Some(Fill::Gradient(g)) => g.stops[stop].color.clone(),
        other => panic!("expected a gradient fill, got {other:?}"),
    }
}

/// The slider on the selected stop (the first) writes a lighter colour, and shows it afterwards.
#[test]
fn the_brightness_slider_lightens_the_selected_stop() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [40.0, 40.0, 120.0, 80.0]})).unwrap()["id"].as_u64().unwrap();
    s.execute("edit.select", &json!({"ids": [id]})).unwrap();
    s.execute("shape.fill", &json!({"gradient": {"stops": [[0.0, "accent1"], [1.0, "accent2"]], "kind": "linear", "angle": 90}})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.pane = Some("format".into());
    app.ui.format_tab = "fill".into();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1100.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    let id = ShapeId(u32::try_from(id).unwrap());
    assert_eq!(brightness_of(&stop_color(&h, id, 0)), 0.0);

    let r = h.get_by_role_and_label(Role::Slider, "Brightness").rect();
    let pos = egui::pos2(r.min.x + r.width() * 0.75, r.center().y);
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
    let lighter = brightness_of(&stop_color(&h, id, 0));
    assert!(lighter > 0.3 && lighter < 0.7, "a click three quarters along lightens the stop, got {lighter}");
    assert_eq!(brightness_of(&stop_color(&h, id, 1)), 0.0, "the other stop is untouched");
}

#[test]
fn a_lone_valueless_transform_is_sent_in_mods() {
    // `Inv` has no number, so the object fields cannot carry it: it goes in `mods` alone.
    let mut c = ColorRef::rgb(deckcraft_color::Rgba::rgb(10, 20, 30));
    c.mods.push(ColorTransform::Inv);
    let v = crate::ribbon::cref_param(&c);
    assert_eq!(v["mods"], json!([{"op": "inv"}]));
}
