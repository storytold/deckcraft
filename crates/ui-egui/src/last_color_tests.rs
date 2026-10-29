//! Shape Fill and Shape Outline remember the last colour picked from their menu, and a click on
//! the button itself applies it again to the next selection, as in PowerPoint (#1080).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use deckcraft_model::{ColorBase, ColorRef, Fill, SchemeSlot, ShapeId};
use egui::{Event, Key, Modifiers, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

/// Two rectangles, the first selected; the second starts red with a blue outline.
fn harness(tab: &str) -> (Harness<'static, SlideApp>, [ShapeId; 2]) {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let mut ids = [ShapeId(0); 2];
    for (i, x) in [100.0, 300.0].iter().enumerate() {
        let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [x, 100.0, 80.0, 60.0]})).unwrap();
        ids[i] = ShapeId(r["id"].as_u64().unwrap() as u32);
    }
    s.execute("selection.select", &json!({"ids": [ids[1].0]})).unwrap();
    s.execute("shape.fill", &json!({"color": "#FF0000"})).unwrap();
    s.execute("shape.line", &json!({"color": "#0000FF", "width": 3})).unwrap();
    s.execute("selection.select", &json!({"ids": [ids[0].0]})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.tab = tab.into();
    let mut h = Harness::builder().with_size(egui::vec2(1800.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    (h, ids)
}

fn select(h: &mut Harness<'static, SlideApp>, id: ShapeId) {
    h.state_mut().session.execute("selection.select", &json!({"ids": [id.0]})).unwrap();
    h.run_steps(3);
}

/// Open the drop-down arrow of a split button, type a hex colour into "More Colors…" and press Enter.
fn pick_hex(h: &mut Harness<'static, SlideApp>, button: &str, hex: &str) {
    // The split button exposes its main and arrow targets separately, named for this command.
    let arrow = h.get_by_role_and_label(egui::accesskit::Role::Button, &format!("{button} options")).rect().center();
    h.input_mut().events.push(Event::PointerMoved(arrow));
    h.run_steps(2);
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos: arrow, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
    h.run_steps(2);
    // The hex field is the text input on the "More Colors… #" label's row, to its right.
    let label = h.get_by_label("More Colors… #").rect();
    let field = h
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .filter(|n| n.rect().min.x >= label.max.x - 1.0 && (n.rect().center().y - label.center().y).abs() < 6.0)
        .map(|n| n.rect().center())
        .next()
        .expect("the More Colors hex field");
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos: field, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
    h.input_mut().events.push(Event::Text(hex.into()));
    h.run_steps(2);
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key { key: Key::Enter, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
    h.run_steps(2);
}

fn fill_rgb(h: &Harness<'static, SlideApp>, id: ShapeId) -> Option<String> {
    let sh = h.state().session.active().unwrap().shape(id).unwrap();
    match sh.fill.as_ref()? {
        Fill::Solid { color: ColorRef { base: ColorBase::Rgb { rgb }, .. } } => Some(rgb.hex()),
        _ => None,
    }
}

fn line_rgb(h: &Harness<'static, SlideApp>, id: ShapeId) -> Option<String> {
    let sh = h.state().session.active().unwrap().shape(id).unwrap();
    match sh.line.as_ref()?.fill.as_ref()? {
        Fill::Solid { color: ColorRef { base: ColorBase::Rgb { rgb }, .. } } => Some(rgb.hex()),
        _ => None,
    }
}

fn line_width(h: &Harness<'static, SlideApp>, id: ShapeId) -> Option<f64> {
    h.state().session.active().unwrap().shape(id).unwrap().line.as_ref()?.width
}

fn fill_reapplies(tab: &str) {
    let (mut h, [a, b]) = harness(tab);
    pick_hex(&mut h, "Shape Fill", "00AA55");
    assert_eq!(fill_rgb(&h, a).as_deref(), Some("00AA55"), "the menu pick applies to the selection");
    select(&mut h, b);
    assert_eq!(fill_rgb(&h, b).as_deref(), Some("FF0000"), "selecting alone changes nothing");
    h.get_by_label("Shape Fill").click();
    h.run_steps(3);
    assert_eq!(fill_rgb(&h, b).as_deref(), Some("00AA55"), "the button applies the last colour, not Accent 1");
}

fn outline_reapplies(tab: &str) {
    let (mut h, [a, b]) = harness(tab);
    pick_hex(&mut h, "Shape Outline", "AA5500");
    assert_eq!(line_rgb(&h, a).as_deref(), Some("AA5500"), "the menu pick applies to the selection");
    select(&mut h, b);
    assert_eq!(line_rgb(&h, b).as_deref(), Some("0000FF"), "selecting alone changes nothing");
    h.get_by_label("Shape Outline").click();
    h.run_steps(3);
    assert_eq!(line_rgb(&h, b).as_deref(), Some("AA5500"), "the button applies the last colour, not Text 1");
    assert_eq!(line_width(&h, b), Some(3.0), "recolouring leaves the weight alone");
}

#[test]
fn home_shape_fill_button_reapplies_the_last_colour() {
    fill_reapplies("Home");
}

#[test]
fn shape_format_shape_fill_button_reapplies_the_last_colour() {
    fill_reapplies("Shape Format");
}

#[test]
fn home_shape_outline_button_reapplies_the_last_colour() {
    outline_reapplies("Home");
}

#[test]
fn shape_format_shape_outline_button_reapplies_the_last_colour() {
    outline_reapplies("Shape Format");
}

#[test]
fn before_any_pick_the_buttons_still_apply_accent_1_and_text_1() {
    let (mut h, [a, _]) = harness("Home");
    h.get_by_label("Shape Fill").click();
    h.run_steps(3);
    h.get_by_label("Shape Outline").click();
    h.run_steps(3);
    let sh = h.state().session.active().unwrap().shape(a).unwrap();
    let slot = |f: &Option<Fill>| match f {
        Some(Fill::Solid { color: ColorRef { base: ColorBase::Scheme { slot }, .. } }) => Some(*slot),
        _ => None,
    };
    assert_eq!(slot(&sh.fill), Some(SchemeSlot::Accent1));
    assert_eq!(slot(&sh.line.as_ref().unwrap().fill), Some(SchemeSlot::Tx1));
}
