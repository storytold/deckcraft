//! Split controls on real ribbon callers: a dropdown-only control must not expose a dead named
//! half, while Group and Rotate keep a real main action beside their options arrow.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use deckcraft_model::ShapeId;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

/// Two rectangles selected on a blank slide, so all the arrange controls are enabled.
fn harness(tab: &str) -> (Harness<'static, SlideApp>, [ShapeId; 2]) {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let mut ids = [ShapeId(0); 2];
    for (i, x) in [100.0, 300.0].iter().enumerate() {
        let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [x, 100.0, 80.0, 60.0]})).unwrap();
        ids[i] = ShapeId(r["id"].as_u64().unwrap() as u32);
    }
    s.execute("selection.select", &json!({"ids": [ids[0].0, ids[1].0]})).unwrap();
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

/// Click the accessible button or chevron with the given name and run three harness steps.
fn click(h: &mut Harness<'static, SlideApp>, name: &str) {
    h.get_by_role_and_label(Role::Button, name).click();
    h.run_steps(3);
}

/// Click each named half, see the same menu, toggle it closed from that same half, then repeat on
/// the other half. The old callers wired only one half, so one full click produced no menu.
fn both_halves_open_same_menu(tab: &str, name: &str, item: &str) {
    let (mut h, _) = harness(tab);
    for half in [name.to_string(), format!("{name} options")] {
        assert!(h.query_by_label(item).is_none(), "{tab}: the {half} menu starts closed");
        click(&mut h, &half);
        assert!(h.query_by_label(item).is_some(), "{tab}: clicking {half} opens the shared menu");
        click(&mut h, &half);
        assert!(h.query_by_label(item).is_none(), "{tab}: clicking {half} again closes the same menu");
    }
}

/// Both the Home Layout label and chevron open and toggle the same layout menu.
#[test]
fn home_layout_opens_the_layout_menu_from_its_label_and_arrow() {
    both_halves_open_same_menu("Home", "Layout", "Title Slide");
}

/// Keyboard entry must recognize either named half as the menu's origin. In particular, Up
/// enters at the last item rather than letting spatial focus navigation escape into the ribbon.
#[test]
fn home_section_menu_enters_wraps_and_closes_from_either_keyboard_trigger() {
    let (mut h, _) = harness("Home");
    for trigger in ["Section", "Section options"] {
        for (entry, first, wrapped) in
            [(egui::Key::ArrowUp, "Remove All Sections", "Add Section"), (egui::Key::ArrowDown, "Add Section", "Remove All Sections")]
        {
            h.get_by_role_and_label(Role::Button, trigger).focus();
            h.run_steps(1);
            assert!(h.get_by_role_and_label(Role::Button, trigger).is_focused(), "{trigger}: the named half has focus");
            h.key_press(egui::Key::Enter);
            h.run_steps(3);
            assert!(h.query_by_role_and_label(Role::Button, "Add Section").is_some(), "{trigger}: Enter opens the menu");

            h.key_press(entry);
            h.run_steps(2);
            assert!(h.get_by_role_and_label(Role::Button, first).is_focused(), "{trigger}: {entry:?} enters the menu");
            // The opposite direction wraps at the edge, then the entry direction wraps back.
            h.key_press(if entry == egui::Key::ArrowUp { egui::Key::ArrowDown } else { egui::Key::ArrowUp });
            h.run_steps(2);
            assert!(h.get_by_role_and_label(Role::Button, wrapped).is_focused(), "{trigger}: focus wraps inside the menu");
            h.key_press(entry);
            h.run_steps(2);
            assert!(h.get_by_role_and_label(Role::Button, first).is_focused(), "{trigger}: focus wraps back");

            h.key_press(egui::Key::Escape);
            h.run_steps(3);
            assert!(h.query_by_role_and_label(Role::Button, "Add Section").is_none(), "{trigger}: Escape closes the menu");
        }
    }
}

/// Main and chevron halves of Shape Effects open the same effects menu on Home and Shape
/// Format.
#[test]
fn shape_effects_opens_the_effects_menu_from_its_label_and_arrow_on_both_tabs() {
    both_halves_open_same_menu("Home", "Shape Effects", "Format Shape…");
    both_halves_open_same_menu("Shape Format", "Shape Effects", "Format Shape…");
}

/// Text Fill's label and chevron both open its colour menu.
#[test]
fn shape_format_text_fill_opens_its_colour_menu_from_its_label_and_arrow() {
    both_halves_open_same_menu("Shape Format", "Text Fill", "More Colors… #");
}

/// The Group main area executes grouping while only its chevron opens the group menu.
#[test]
fn shape_format_group_main_groups_and_its_arrow_opens_the_group_menu() {
    let (mut h, _) = harness("Shape Format");
    click(&mut h, "Group");
    {
        let d = h.state().session.active().unwrap();
        assert_eq!(d.selection.shapes.len(), 1, "the main half grouped the two selected shapes");
        let grouped = d.shape(d.selection.shapes[0]).unwrap();
        assert!(grouped.is_group(), "the result is a group, not a menu choice");
    }
    click(&mut h, "Group options");
    assert!(h.query_by_label("Ungroup").is_some(), "the arrow opens the Group actions");
}

/// The Rotate main area rotates while its chevron opens the rotate menu.
#[test]
fn shape_format_rotate_main_rotates_and_its_arrow_opens_the_rotate_menu() {
    let (mut h, ids) = harness("Shape Format");
    h.state_mut().session.execute("selection.select", &json!({"ids": [ids[0].0]})).unwrap();
    h.run_steps(2);
    click(&mut h, "Rotate");
    let rot = h.state().session.active().unwrap().shape(ids[0]).unwrap().xfrm.unwrap().rot;
    assert!((rot - 90.0).abs() < 1e-9, "the main half rotates right: {rot}");
    click(&mut h, "Rotate options");
    assert!(h.query_by_label("Rotate Left 90°").is_some(), "the arrow opens the Rotate actions");
}
