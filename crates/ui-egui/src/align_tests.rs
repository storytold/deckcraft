//! Arrange ▸ Align: Align to Slide and Align Selected Objects, as in PowerPoint. One object aligns
//! to the slide and several to each other, until the other choice is picked for that selection.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use deckcraft_model::ShapeId;
use egui::accesskit::Toggled;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

use crate::{Services, SlideApp};

/// A blank slide with three rectangles at different places; `select` picks which are selected.
fn harness(select: &[usize]) -> (Harness<'static, SlideApp>, Vec<ShapeId>) {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let ids: Vec<ShapeId> = [[100.0, 100.0], [300.0, 200.0], [500.0, 250.0]]
        .iter()
        .map(|[x, y]| {
            let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [x, y, 80.0, 60.0]})).unwrap();
            ShapeId(r["id"].as_u64().unwrap() as u32)
        })
        .collect();
    let picked: Vec<u32> = select.iter().map(|&i| ids[i].0).collect();
    s.execute("selection.select", &json!({"ids": picked})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.tab = "Shape Format".into();
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

fn pick(h: &mut Harness<'_, SlideApp>, item: &str) {
    h.get_by_label("Align").click();
    h.run_steps(2);
    h.get_by_label(item).click();
    h.run_steps(2);
}

fn marked(h: &mut Harness<'_, SlideApp>) -> (bool, bool) {
    h.get_by_label("Align").click();
    h.run_steps(2);
    let m = (
        h.get_by_label("Align to Slide").accesskit_node().toggled() == Some(Toggled::True),
        h.get_by_label("Align Selected Objects").accesskit_node().toggled() == Some(Toggled::True),
    );
    // Esc would also clear the selection: a second click on Align closes the menu.
    h.get_by_label("Align").click();
    h.run_steps(2);
    m
}

fn pos(h: &Harness<'_, SlideApp>, id: ShapeId) -> (f64, f64) {
    let x = h.state().session.active().unwrap().shape(id).unwrap().xfrm.unwrap();
    (x.x, x.y)
}

#[test]
fn align_to_slide_moves_align_left_to_the_slide_edge() {
    let (mut h, ids) = harness(&[0, 1]);
    pick(&mut h, "Align to Slide");
    pick(&mut h, "Align Left");
    assert_eq!(pos(&h, ids[0]).0, 0.0);
    assert_eq!(pos(&h, ids[1]).0, 0.0);
}

#[test]
fn one_object_aligns_to_the_slide_and_two_to_each_other() {
    let (mut h, ids) = harness(&[1]);
    assert_eq!(marked(&mut h), (true, false));
    pick(&mut h, "Align Left");
    assert_eq!(pos(&h, ids[1]).0, 0.0);

    let (mut h, ids) = harness(&[0, 1]);
    assert_eq!(marked(&mut h), (false, true));
    pick(&mut h, "Align Left");
    assert_eq!(pos(&h, ids[1]).0, 100.0, "the left edge of the selection");
}

#[test]
fn the_choice_stays_until_the_selection_changes() {
    let (mut h, ids) = harness(&[0, 1]);
    pick(&mut h, "Align to Slide");
    pick(&mut h, "Align Left");
    pick(&mut h, "Align Top");
    assert_eq!(pos(&h, ids[0]), (0.0, 0.0));
    assert_eq!(pos(&h, ids[1]), (0.0, 0.0));
    assert_eq!(marked(&mut h), (true, false));

    // Another selection starts again from its own default.
    h.state_mut().run("selection.select", json!({"ids": [ids[1].0, ids[2].0]})).unwrap();
    h.run_steps(2);
    assert_eq!(marked(&mut h), (false, true));
    pick(&mut h, "Align Bottom");
    assert_eq!(pos(&h, ids[1]).1, 250.0, "the bottom of the selection, not of the slide");
}
