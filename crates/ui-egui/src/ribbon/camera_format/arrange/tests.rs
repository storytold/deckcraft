//! Camera Format ▸ Arrange: the same Arrange group as a picture's, acting on the Cameo with the
//! shared order, Selection Pane, align, group and rotate commands.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use deckcraft_model::{Shape, ShapeKind};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

/// A blank slide holding the shapes `build` adds; `select` picks which are selected.
/// The harness plus the ids the build made, in document order: a test selects by index and then
/// needs the id it picked.
fn harness(build: impl FnOnce(&mut Session) -> Vec<u32>, select: &[u32]) -> (Harness<'static, SlideApp>, Vec<u32>) {
    let mut s = Session::with_new();
    s.execute("slide.layout", &json!({"layout": "blank"})).unwrap();
    let ids = build(&mut s);
    let picked: Vec<u32> = select.iter().map(|&i| ids[usize::try_from(i).unwrap()]).collect();
    s.execute("selection.select", &json!({"ids": picked})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.tab = "Camera Format".into();
    let mut h = Harness::builder().with_size(egui::vec2(1920.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    (h, ids)
}

fn cameo(s: &mut Session, x: f64) -> u32 {
    u32::try_from(s.execute("insert.cameo", &json!({"rect": [x, 100.0, 162.0, 162.0]})).unwrap()["id"].as_u64().unwrap()).unwrap()
}

fn rect(s: &mut Session, x: f64) -> u32 {
    u32::try_from(s.execute("shape.insert", &json!({"preset": "rect", "rect": [x, 100.0, 80.0, 60.0]})).unwrap()["id"].as_u64().unwrap()).unwrap()
}

fn order(h: &Harness<'_, SlideApp>) -> Vec<u32> {
    h.state().session.active().unwrap().doc.slides[0].shapes.iter().map(|s| s.id.0).collect()
}

fn shape(h: &Harness<'_, SlideApp>, id: u32) -> Shape {
    h.state().session.active().unwrap().doc.slides[0].shape(deckcraft_model::ShapeId(id)).expect("shape").clone()
}

fn undos(h: &Harness<'_, SlideApp>) -> usize {
    h.state().session.active().unwrap().history.undo.len()
}

/// Open `opener` (a button or a split button's chevron) and click the menu item `item`.
fn pick(h: &mut Harness<'_, SlideApp>, opener: &str, item: &str) {
    h.get_by_label(opener).click();
    h.run_steps(2);
    h.get_by_label(item).click();
    h.run_steps(2);
}

#[test]
fn the_arrange_section_draws_every_control_for_a_cameo() {
    for width in [1600.0, 1920.0] {
        let (h, _) = {
            let mut s = Session::with_new();
            s.execute("slide.layout", &json!({"layout": "blank"})).unwrap();
            let id = cameo(&mut s, 200.0);
            s.execute("selection.select", &json!({"ids": [id]})).unwrap();
            let mut app = SlideApp::new(s, Services::default());
            app.ui.tab = "Camera Format".into();
            let mut h = Harness::builder().with_size(egui::vec2(width, 900.0)).build_ui_state(
                |ui, app: &mut SlideApp| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                },
                app,
            );
            h.run_steps(4);
            (h, id)
        };
        for label in [
            "Bring Forward",
            "Bring Forward options",
            "Send Backward",
            "Send Backward options",
            "Selection Pane",
            "Align",
            "Group",
            "Group options",
            "Rotate",
            "Rotate options",
        ] {
            assert!(h.query_by_label(label).is_some(), "{label} missing at {width} px");
        }
    }
}

#[test]
fn bring_forward_steps_once_and_the_menu_brings_to_front() {
    let (mut h, _) = harness(
        |s| {
            let a = cameo(s, 200.0);
            let b = rect(s, 400.0);
            let c = rect(s, 600.0);
            vec![a, b, c]
        },
        &[0],
    );
    let (a, b, c) = (order(&h)[0], order(&h)[1], order(&h)[2]);
    h.get_by_label("Bring Forward").click();
    h.run_steps(2);
    assert_eq!(order(&h), vec![b, a, c], "one step forward");
    pick(&mut h, "Bring Forward options", "Bring to Front");
    assert_eq!(order(&h), vec![b, c, a], "the menu brings the cameo to the front");
    // Already at the front: no change and no undo step.
    let undo = undos(&h);
    h.get_by_label("Bring Forward").click();
    h.run_steps(2);
    assert_eq!(order(&h), vec![b, c, a]);
    assert_eq!(undos(&h), undo);
}

#[test]
fn send_backward_steps_once_and_the_menu_sends_to_back() {
    let (mut h, _) = harness(
        |s| {
            let a = rect(s, 200.0);
            let b = rect(s, 400.0);
            let c = cameo(s, 600.0);
            vec![a, b, c]
        },
        &[2],
    );
    let (a, b, c) = (order(&h)[0], order(&h)[1], order(&h)[2]);
    h.get_by_label("Send Backward").click();
    h.run_steps(2);
    assert_eq!(order(&h), vec![a, c, b], "one step back");
    pick(&mut h, "Send Backward options", "Send to Back");
    assert_eq!(order(&h), vec![c, a, b], "the menu sends the cameo to the back");
    // Already at the back: no change and no undo step.
    let undo = undos(&h);
    h.get_by_label("Send Backward").click();
    h.run_steps(2);
    assert_eq!(order(&h), vec![c, a, b]);
    assert_eq!(undos(&h), undo);
}

#[test]
fn the_selection_pane_opens_on_the_cameo_and_lists_it() {
    let (mut h, id) = {
        let mut s = Session::with_new();
        s.execute("slide.layout", &json!({"layout": "blank"})).unwrap();
        let id = cameo(&mut s, 200.0);
        s.execute("selection.select", &json!({"ids": [id]})).unwrap();
        let mut app = SlideApp::new(s, Services::default());
        app.ui.tab = "Camera Format".into();
        let mut h = Harness::builder().with_size(egui::vec2(1920.0, 900.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(4);
        (h, id)
    };
    let undo = undos(&h);
    h.get_by_label("Selection Pane").click();
    h.run_steps(2);
    assert_eq!(h.state().ui.pane.as_deref(), Some("selection"));
    assert_eq!(undos(&h), undo, "opening a pane is not a document change");
    let st = h.state().session.active().unwrap();
    let rows = crate::selpane::rows(&st.doc.slides[0].shapes, &Default::default());
    let row = rows.iter().find(|r| r.id == id).expect("the camera row");
    assert!(row.name.starts_with("Camera"), "{}", row.name);
    assert!(row.parent.is_none());
}

#[test]
fn align_moves_the_cameo_to_the_slide_edges() {
    let (mut h, id) = {
        let mut s = Session::with_new();
        s.execute("slide.layout", &json!({"layout": "blank"})).unwrap();
        let id = cameo(&mut s, 200.0);
        s.execute("selection.select", &json!({"ids": [id]})).unwrap();
        let mut app = SlideApp::new(s, Services::default());
        app.ui.tab = "Camera Format".into();
        let mut h = Harness::builder().with_size(egui::vec2(1920.0, 900.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(4);
        (h, id)
    };
    let xfrm = shape(&h, id).xfrm.expect("cameo box");
    assert!(xfrm.x > 0.0 && xfrm.y > 0.0);
    pick(&mut h, "Align", "Align Left");
    assert_eq!(shape(&h, id).xfrm.expect("box").x, 0.0);
    pick(&mut h, "Align", "Align Top");
    assert_eq!(shape(&h, id).xfrm.expect("box").y, 0.0);
}

#[test]
fn the_cameo_groups_with_another_shape_and_keeps_its_marker() {
    let (mut h, ids) = harness(
        |s| {
            let a = cameo(s, 200.0);
            let b = rect(s, 400.0);
            vec![a, b]
        },
        &[0, 1],
    );
    let (cameo_id, rect_id) = (ids[0], ids[1]);
    let undo = undos(&h);
    h.get_by_label("Group").click();
    h.run_steps(2);
    assert_eq!(undos(&h), undo + 1, "one undoable group");
    let group = h.state().session.active().unwrap().doc.slides[0].shapes.iter().find(|s| s.is_group()).cloned().expect("group");
    let ShapeKind::Group { children, .. } = &group.kind else { panic!("not a group") };
    assert_eq!(children.iter().map(|c| c.id.0).collect::<Vec<_>>(), vec![cameo_id, rect_id]);
    assert!(children[0].is_cameo(), "the Cameo marker stays on the child");
    // The group, not a Cameo, is selected now: the menu still offers all three commands, with
    // Ungroup and Regroup disabled until there is a group to undo.
    let (mut single, _) = harness(|s| vec![cameo(s, 200.0)], &[0]);
    single.get_by_label("Group options").click();
    single.run_steps(2);
    // The ribbon keeps its own button beside the menu's copy, so count rather than get.
    for label in ["Group", "Ungroup", "Regroup"] {
        assert!(single.query_all_by_label(label).count() >= 1, "{label} in the Group menu");
    }
}

#[test]
fn rotate_turns_the_cameo_and_the_menu_flips_it() {
    let (mut h, id) = {
        let mut s = Session::with_new();
        s.execute("slide.layout", &json!({"layout": "blank"})).unwrap();
        let id = cameo(&mut s, 200.0);
        s.execute("selection.select", &json!({"ids": [id]})).unwrap();
        let mut app = SlideApp::new(s, Services::default());
        app.ui.tab = "Camera Format".into();
        let mut h = Harness::builder().with_size(egui::vec2(1920.0, 900.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(4);
        (h, id)
    };
    assert_eq!(shape(&h, id).xfrm.expect("box").rot, 0.0);
    h.get_by_label("Rotate").click();
    h.run_steps(2);
    assert_eq!(shape(&h, id).xfrm.expect("box").rot, 90.0);
    pick(&mut h, "Rotate options", "Rotate Left 90°");
    assert_eq!(shape(&h, id).xfrm.expect("box").rot, 0.0);
    pick(&mut h, "Rotate options", "Flip Horizontal");
    assert!(shape(&h, id).xfrm.expect("box").flip_h);
    pick(&mut h, "Rotate options", "More Rotation Options…");
    assert_eq!(h.state().ui.pane.as_deref(), Some("format"));
    assert_eq!(h.state().ui.format_tab, "size", "the rotation field lives on Size & Position");
}
