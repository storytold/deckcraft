//! Format Shape and Format Background ▸ Pattern fill: the preset gallery and the Foreground and
//! Background color menus.

use deckcraft_color::Rgba;
use deckcraft_engine::Session;
use deckcraft_model::style::PatternFill;
use deckcraft_model::{Background, ColorBase, ColorRef, Fill, ShapeId};
use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

fn harness(mut app: SlideApp, pane: &str) -> Harness<'static, SlideApp> {
    app.ui.pane = Some(pane.into());
    let mut h = Harness::builder().with_size(egui::vec2(1800.0, 1100.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

/// A new deck with one rectangle selected and filled with `pct25` in accent 1 on background 1.
fn patterned_shape() -> (Session, ShapeId) {
    let mut s = Session::with_new();
    let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, 80.0, 200.0, 120.0]})).unwrap();
    let id = r.get("id").and_then(|v| v.as_u64()).unwrap();
    s.execute("edit.select", &json!({"ids": [id]})).unwrap();
    s.execute("shape.fill", &json!({"pattern": {"preset": "pct25", "fg": "accent1", "bg": "bg1"}})).unwrap();
    (s, ShapeId(u32::try_from(id).unwrap()))
}

fn shape_pattern(h: &Harness<'_, SlideApp>, id: ShapeId) -> PatternFill {
    match &h.state().session.active().unwrap().shape(id).unwrap().fill {
        Some(Fill::Pattern(pt)) => pt.clone(),
        other => panic!("expected a pattern fill, got {other:?}"),
    }
}

fn background_pattern(h: &Harness<'_, SlideApp>) -> PatternFill {
    match &h.state().session.active().unwrap().doc.slides[0].background {
        Some(Background::Fill { fill: Fill::Pattern(pt) }) => pt.clone(),
        other => panic!("expected a pattern background, got {other:?}"),
    }
}

/// Open the color menu named `button` and pick `hex` from its More Colors field.
fn pick_hex(h: &mut Harness<'_, SlideApp>, button: &str, hex: &str) {
    h.get_by_label(button).click();
    h.run_steps(2);
    // The hex field is the text input on the "More Colors… #" label's row, to its right.
    let label = h.get_by_label("More Colors… #").rect();
    let field = h
        .get_all_by_role(Role::TextInput)
        .find(|n| n.rect().min.x >= label.max.x - 1.0 && (n.rect().center().y - label.center().y).abs() < 6.0)
        .map(|n| n.rect().center())
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

#[test]
fn shape_pattern_fill_picks_a_preset_from_the_gallery() {
    let (s, id) = patterned_shape();
    let mut h = harness(SlideApp::new(s, Services::default()), "format");
    let _ = h.get_by_label("Pattern fill");
    assert!(h.query_by_label("Transparency").is_none(), "a pattern has no solid-fill transparency");
    assert_eq!(crate::fillui::pattern_gallery().count(), 48);
    for name in crate::fillui::pattern_gallery() {
        assert!(deckcraft_render::pattern_presets().any(|p| p == name), "{name}");
        let _ = h.get_by_label(crate::fillui::pattern_label(name));
    }
    for label in ["Horizontal", "Vertical", "Cross", "Downward diagonal", "Upward diagonal", "Diagonal cross"] {
        assert!(h.query_by_label(label).is_none(), "{label} is not in the gallery");
    }
    let undo = h.state().session.active().unwrap().history.undo.len();
    h.get_by_label("Dark downward diagonal").click();
    h.run_steps(2);
    let pt = shape_pattern(&h, id);
    assert_eq!(pt.preset, "dkDnDiag");
    assert_eq!((pt.fg, pt.bg), (ColorRef::scheme(deckcraft_color::SchemeSlot::Accent1), ColorRef::scheme(deckcraft_color::SchemeSlot::Bg1)));
    assert_eq!(h.state().session.active().unwrap().history.undo.len(), undo + 1, "one undo step");
}

#[test]
fn shape_pattern_fill_keeps_preset_and_system_colors() {
    let (mut s, id) = patterned_shape();
    let fg = json!({"preset": "red", "lumMod": 50000});
    let bg = json!({"system": "window", "last": "#F0F0F0"});
    s.execute("shape.fill", &json!({"pattern": {"preset": "pct25", "fg": fg, "bg": bg}})).unwrap();
    let mut h = harness(SlideApp::new(s, Services::default()), "format");
    let before = shape_pattern(&h, id);
    assert!(matches!(before.fg.base, ColorBase::Preset { .. }) && matches!(before.bg.base, ColorBase::System { .. }));
    h.get_by_label("Zig zag").click();
    h.run_steps(2);
    let pt = shape_pattern(&h, id);
    assert_eq!(pt.preset, "zigZag");
    assert_eq!((&pt.fg, &pt.bg), (&before.fg, &before.bg), "a preset change keeps both colors");
    pick_hex(&mut h, "Foreground color", "00FF00");
    assert_eq!(shape_pattern(&h, id).bg, before.bg, "a foreground change keeps the background");
}

#[test]
fn pattern_gallery_shows_which_swatch_has_keyboard_focus() {
    let (s, _) = patterned_shape();
    let mut h = harness(SlideApp::new(s, Services::default()), "format");
    let r = h.get_by_label("Zig zag").rect();
    let accent = crate::theme::Tokens::get(&h.ctx).accent;
    let outlined = |h: &Harness<'_, SlideApp>| {
        h.output().shapes.iter().any(|c| match &c.shape {
            egui::Shape::Rect(rs) => {
                rs.stroke.width == 2.0 && rs.stroke.color == accent && (rs.rect.center() - r.center()).length() < 0.5 && rs.rect.width() < r.width()
            }
            _ => false,
        })
    };
    assert!(!outlined(&h), "an unselected swatch has a plain border");
    h.get_by_label("Zig zag").focus();
    h.run_steps(2);
    assert!(h.get_by_label("Zig zag").is_focused());
    assert!(outlined(&h), "the focused swatch is outlined");
}

/// A solid fill shows its transparency slider and does not expose a pattern gallery.
#[test]
fn shape_solid_fill_keeps_its_transparency_without_a_pattern_gallery() {
    let (mut s, _) = patterned_shape();
    s.execute("shape.fill", &json!({"color": "accent2"})).unwrap();
    let h = harness(SlideApp::new(s, Services::default()), "format");
    // 0.37 exposes the slider's numeric entry as a SpinButton with the same name; the slider is
    // the control the user drags.
    let _ = h.get_by_role_and_label(Role::Slider, "Transparency");
    assert!(h.query_by_label("Foreground color").is_none());
    assert!(h.query_by_label("Zig zag").is_none());
}

#[test]
fn shape_pattern_fill_sets_its_foreground_and_background_colors() {
    let (s, id) = patterned_shape();
    let mut h = harness(SlideApp::new(s, Services::default()), "format");
    pick_hex(&mut h, "Foreground color", "00FF00");
    let pt = shape_pattern(&h, id);
    assert_eq!(pt.fg, ColorRef::rgb(Rgba::from_hex("00FF00").unwrap()));
    assert_eq!(pt.preset, "pct25", "the pattern stays");
    pick_hex(&mut h, "Background color", "112233");
    let pt = shape_pattern(&h, id);
    assert_eq!(pt.bg, ColorRef::rgb(Rgba::from_hex("112233").unwrap()));
    assert_eq!(pt.fg, ColorRef::rgb(Rgba::from_hex("00FF00").unwrap()), "the foreground stays");
}

#[test]
fn background_pattern_fill_offers_every_preset_and_both_colors() {
    let mut s = Session::with_new();
    s.execute("design.background", &json!({"pattern": {"preset": "pct20", "fg": "accent1", "bg": "bg1"}})).unwrap();
    let mut h = harness(SlideApp::new(s, Services::default()), "background");
    h.get_by_label("Zig zag").click();
    h.run_steps(2);
    assert_eq!(background_pattern(&h).preset, "zigZag");
    pick_hex(&mut h, "Background color", "FFEE00");
    let pt = background_pattern(&h);
    assert_eq!((pt.preset.as_str(), pt.bg), ("zigZag", ColorRef::rgb(Rgba::from_hex("FFEE00").unwrap())));
}

#[test]
fn background_pattern_fill_keeps_a_system_color() {
    let mut s = Session::with_new();
    let bg = json!({"system": "window", "last": "#F0F0F0"});
    s.execute("design.background", &json!({"pattern": {"preset": "pct20", "fg": "accent1", "bg": bg}})).unwrap();
    let mut h = harness(SlideApp::new(s, Services::default()), "background");
    let before = background_pattern(&h).bg;
    assert!(matches!(before.base, ColorBase::System { .. }));
    h.get_by_label("Zig zag").click();
    h.run_steps(2);
    let pt = background_pattern(&h);
    assert_eq!((pt.preset.as_str(), pt.bg), ("zigZag", before));
}
