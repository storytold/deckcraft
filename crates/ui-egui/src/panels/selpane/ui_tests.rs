//! The Selection pane driven like a person would: clicks, Ctrl+click, drag and drop, the eye,
//! the padlock, renaming and the keyboard.

use deckcraft_engine::Session;
use egui::{Modifiers, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

/// A blank slide with A, B, C (back to front) and the group G = [D, E] in front; the pane open.
fn harness() -> (Harness<'static, SlideApp>, Vec<u32>) {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let mut ids = vec![];
    for (i, n) in ["A", "B", "C", "D", "E"].iter().enumerate() {
        let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [20.0 + 110.0 * i as f64, 50, 100, 80]})).unwrap();
        let id = r["id"].as_u64().unwrap() as u32;
        s.execute("shape.rename", &json!({"id": id, "name": n})).unwrap();
        ids.push(id);
    }
    let g = s.execute("arrange.group", &json!({"ids": [ids[3], ids[4]]})).unwrap()["id"].as_u64().unwrap() as u32;
    s.execute("shape.rename", &json!({"id": g, "name": "G"})).unwrap();
    s.execute("edit.select", &json!({"ids": []})).unwrap();
    ids.push(g);
    let mut app = SlideApp::new(s, Services::default());
    app.ui.pane = Some("selection".into());
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    (h, ids)
}

fn names(h: &Harness<'_, SlideApp>) -> String {
    fn rec(v: &[deckcraft_model::Shape]) -> String {
        v.iter().map(|x| if x.is_group() { format!("{}[{}]", x.name, rec(x.children())) } else { x.name.clone() }).collect::<Vec<_>>().join(" ")
    }
    rec(h.state().session.active().unwrap().shapes())
}

fn selected(h: &Harness<'_, SlideApp>) -> Vec<String> {
    let st = h.state().session.active().unwrap();
    st.selection.shapes.iter().filter_map(|i| st.shape(*i)).map(|s| s.name.clone()).collect()
}

fn row(h: &Harness<'_, SlideApp>, name: &str) -> egui::Rect {
    h.get_by_label(name).rect()
}

fn shape<'a>(h: &'a Harness<'_, SlideApp>, name: &str) -> &'a deckcraft_model::Shape {
    let st = h.state().session.active().unwrap();
    let mut found = None;
    deckcraft_model::walk(st.shapes(), &mut |s, _| {
        if found.is_none() && s.name == name {
            found = Some(s);
        }
    });
    found.unwrap()
}

#[test]
fn lists_front_most_first_with_groups_expanded() {
    let (h, _) = harness();
    let ys: Vec<f32> = ["G", "D", "E", "C", "B", "A"].iter().map(|n| row(&h, n).center().y).collect();
    // G is in front, then its children front-most first (E above D), then C, B, A.
    let order = ["G", "E", "D", "C", "B", "A"];
    let mut by_y: Vec<(&str, f32)> = ["G", "D", "E", "C", "B", "A"].iter().copied().zip(ys).collect();
    by_y.sort_by(|a, b| a.1.total_cmp(&b.1));
    assert_eq!(by_y.iter().map(|x| x.0).collect::<Vec<_>>(), order);
    // Children are indented under the group (the name's row starts at the same x; the group
    // has a fold button).
    assert!(h.query_by_label("Collapse G").is_some());
    assert!(h.query_by_label("Hide A").is_some() && h.query_by_label("Lock A").is_some());
}

#[test]
fn click_ctrl_click_and_shift_click_select() {
    let (mut h, _) = harness();
    h.get_by_label("A").click();
    h.run_steps(2);
    assert_eq!(selected(&h), ["A"]);
    h.get_by_label("C").click_modifiers(Modifiers::COMMAND);
    h.run_steps(2);
    assert_eq!(selected(&h), ["A", "C"]);
    h.get_by_label("A").click_modifiers(Modifiers::COMMAND);
    h.run_steps(2);
    assert_eq!(selected(&h), ["C"]);
    // Shift+click: the run from the last click (C) down to A.
    h.get_by_label("C").click();
    h.run_steps(2);
    h.get_by_label("A").click_modifiers(Modifiers::SHIFT);
    h.run_steps(2);
    let mut sel = selected(&h);
    sel.sort();
    assert_eq!(sel, ["A", "B", "C"]);
    // A child of the group.
    h.get_by_label("D").click();
    h.run_steps(2);
    assert_eq!(selected(&h), ["D"]);
}

#[test]
fn eye_and_padlock_toggle_and_undo() {
    let (mut h, _) = harness();
    h.get_by_label("Hide B").click();
    h.run_steps(2);
    assert!(shape(&h, "B").hidden);
    assert!(h.query_by_label("Show B").is_some(), "the eye now offers to show it");
    h.get_by_label("Lock G").click();
    h.run_steps(2);
    assert!(shape(&h, "G").locked && shape(&h, "D").locked && shape(&h, "E").locked, "the group locks with its children");
    h.get_by_label("Unlock D").click();
    h.run_steps(2);
    assert!(!shape(&h, "D").locked && shape(&h, "G").locked);
    h.get_by_label("Hide All").click();
    h.run_steps(2);
    assert!(["A", "B", "C", "G"].iter().all(|n| shape(&h, n).hidden));
    h.get_by_label("Show All").click();
    h.run_steps(2);
    assert!(["A", "B", "C", "G", "D", "E"].iter().all(|n| !shape(&h, n).hidden));
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(2);
    assert!(shape(&h, "A").hidden);
}

/// Drag a row with the mouse: press, move in steps, release.
fn drag(h: &mut Harness<'_, SlideApp>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.run_steps(1);
    h.drag_at(from);
    h.run_steps(1);
    for i in 1..=6 {
        let t = i as f32 / 6.0;
        h.hover_at(from + (to - from) * t);
        h.run_steps(1);
    }
    h.drop_at(to);
    h.run_steps(3);
}

#[test]
fn drag_and_drop_restacks_among_siblings() {
    let (mut h, _) = harness();
    assert_eq!(names(&h), "A B C G[D E]");
    // A (bottom row) dragged above G (top row): A goes to the front.
    let (a, g) = (row(&h, "A"), row(&h, "G"));
    drag(&mut h, a.center(), Pos2::new(g.center().x, g.top() + 2.0));
    assert_eq!(names(&h), "B C G[D E] A");
    assert_eq!(selected(&h), ["A"], "the dragged object is selected");
    // C dragged to the bottom of the list: the back.
    let (c, b) = (row(&h, "C"), row(&h, "B"));
    drag(&mut h, c.center(), Pos2::new(b.center().x, b.bottom() + 30.0));
    assert_eq!(names(&h), "C B G[D E] A");
    // D (a child) dragged far above its group stays in the group, at its front.
    let (d, a) = (row(&h, "D"), row(&h, "A"));
    drag(&mut h, d.center(), Pos2::new(d.center().x, a.top() - 10.0));
    assert_eq!(names(&h), "C B G[E D] A");
    // B dropped over the group lands among the top-level objects (here just in front of the
    // group), never inside it.
    let (b, g) = (row(&h, "B"), row(&h, "G"));
    drag(&mut h, b.center(), Pos2::new(g.center().x, g.center().y + 6.0));
    assert_eq!(names(&h), "C G[E D] B A");
    // Each drop is one undo step.
    for _ in 0..4 {
        h.state_mut().run("edit.undo", json!({})).unwrap();
    }
    assert_eq!(names(&h), "A B C G[D E]");
}

#[test]
fn bring_forward_and_send_backward_buttons() {
    let (mut h, _) = harness();
    h.get_by_label("A").click();
    h.run_steps(2);
    h.get_by_label("Bring Forward").click();
    h.run_steps(2);
    assert_eq!(names(&h), "B A C G[D E]");
    h.get_by_label("D").click();
    h.run_steps(2);
    h.get_by_label("Bring Forward").click();
    h.run_steps(2);
    h.get_by_label("Bring Forward").click();
    h.run_steps(2);
    assert_eq!(names(&h), "B A C G[E D]", "a child stays in its group");
    h.get_by_label("Send Backward").click();
    h.run_steps(2);
    assert_eq!(names(&h), "B A C G[D E]");
}

#[test]
fn double_click_renames_in_place() {
    let (mut h, _) = harness();
    let b = row(&h, "B").center();
    h.hover_at(b);
    h.run_steps(1);
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: b, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            h.run_steps(1);
        }
    }
    h.run_steps(2);
    assert!(h.state().selpane.renaming.is_some(), "double-click starts renaming");
    // The whole name is selected: typing replaces it.
    h.event(egui::Event::Text("Bravo".into()));
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(shape(&h, "Bravo").id, shape(&h, "Bravo").id);
    assert!(h.state().selpane.renaming.is_none());
    // Escape cancels.
    h.get_by_label("Bravo").click();
    h.run_steps(2);
    h.key_press(egui::Key::F2);
    h.run_steps(3);
    assert!(h.state().selpane.renaming.is_some(), "F2 renames the selected object");
    h.event(egui::Event::Text("Nope".into()));
    h.run_steps(1);
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.query_by_label("Bravo").is_some());
    assert!(h.query_by_label("Nope").is_none());
}

#[test]
fn arrow_keys_move_through_the_list_and_collapse_hides_children() {
    let (mut h, _) = harness();
    h.get_by_label("G").click();
    h.run_steps(2);
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(2);
    assert_eq!(selected(&h), ["E"]);
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(2);
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(2);
    assert_eq!(selected(&h), ["C"]);
    // The arrows moved through the list: C itself didn't move on the slide.
    assert_eq!(shape(&h, "C").xfrm.map(|x| x.x), Some(240.0));
    h.key_press(egui::Key::ArrowUp);
    h.run_steps(2);
    assert_eq!(selected(&h), ["D"]);
    h.get_by_label("Collapse G").click();
    h.run_steps(2);
    assert!(h.query_by_label("D").is_none() && h.query_by_label("Expand G").is_some());
    h.get_by_label("Expand G").click();
    h.run_steps(2);
    assert!(h.query_by_label("D").is_some());
}
