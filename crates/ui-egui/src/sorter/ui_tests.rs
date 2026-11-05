//! Slide Sorter sections: add, rename, collapse, and dragging a whole section.

use deckcraft_engine::Session;
use egui::Pos2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

fn harness(with_sections: bool) -> Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    for _ in 0..3 {
        s.execute("slide.new", &json!({})).unwrap();
    }
    if with_sections {
        s.execute("section.add", &json!({"name": "Alpha", "at": 0})).unwrap();
        s.execute("section.add", &json!({"name": "Beta", "at": 2})).unwrap();
    }
    let mut app = SlideApp::new(s, Services::default());
    app.ui.view = crate::ViewMode::Sorter;
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn section_names(h: &Harness<'_, SlideApp>) -> Vec<String> {
    h.state().session.active().unwrap().doc.sections.iter().map(|s| s.name.clone()).collect()
}

fn section_slides(h: &Harness<'_, SlideApp>) -> Vec<Vec<u32>> {
    h.state().session.active().unwrap().doc.sections.iter().map(|s| s.slides.iter().map(|id| id.0).collect()).collect()
}

fn double_click(h: &mut Harness<'_, SlideApp>, p: Pos2) {
    h.hover_at(p);
    h.run_steps(1);
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
            h.run_steps(1);
        }
    }
    h.run_steps(2);
}

/// Presses at `from`, moves to `to` in a few steps and releases there.
pub(crate) fn drag(h: &mut Harness<'_, SlideApp>, from: Pos2, to: Pos2) {
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
fn drop_slot_and_move_dest() {
    assert_eq!(super::drop_slot(&[10.0, 40.0, 70.0], 0.0), 0);
    assert_eq!(super::drop_slot(&[10.0, 40.0, 70.0], 20.0), 1);
    assert_eq!(super::drop_slot(&[10.0, 40.0, 70.0], 100.0), 3);
    assert_eq!(super::move_dest(1, 1), None);
    assert_eq!(super::move_dest(1, 2), None);
    assert_eq!(super::move_dest(1, 0), Some(0));
    assert_eq!(super::move_dest(0, 2), Some(1));
}

#[test]
fn add_section_splits_at_the_current_slide() {
    let mut h = harness(false);
    assert!(h.query_by_label("Slide 4").is_some());
    assert!(section_names(&h).is_empty());
    h.get_by_label("Add Section").click();
    h.run_steps(3);
    assert_eq!(section_names(&h), ["Default Section", "Untitled Section"]);
    let slides = section_slides(&h);
    assert_eq!(slides[0].len(), 3);
    assert_eq!(slides[1].len(), 1);
    assert!(h.query_by_label("Default Section").is_some());
    assert!(h.query_by_label("Untitled Section").is_some());
    assert!(h.query_by_label("Slide 1").is_some() && h.query_by_label("Slide 4").is_some());
}

#[test]
fn collapse_hides_that_sections_slides_and_expand_brings_them_back() {
    let mut h = harness(true);
    assert_eq!(section_names(&h), ["Alpha", "Beta"]);
    assert_eq!(section_slides(&h).iter().map(|s| s.len()).collect::<Vec<_>>(), [2, 2]);
    assert!(h.query_by_label("Slide 1").is_some() && h.query_by_label("Slide 4").is_some());
    h.get_by_label("Collapse Beta").click();
    h.run_steps(2);
    assert!(h.query_by_label("Slide 1").is_some() && h.query_by_label("Slide 2").is_some());
    assert!(h.query_by_label("Slide 3").is_none() && h.query_by_label("Slide 4").is_none());
    assert!(h.query_by_label("Expand Beta").is_some());
    h.get_by_label("Expand Beta").click();
    h.run_steps(2);
    assert!(h.query_by_label("Slide 3").is_some() && h.query_by_label("Slide 4").is_some());
}

#[test]
fn double_click_renames_and_switching_documents_drops_the_draft() {
    let mut h = harness(true);
    let beta = h.get_by_label("Beta").rect().center();
    double_click(&mut h, beta);
    assert!(h.state().sorter.renaming.is_some(), "double-click starts renaming");
    h.event(egui::Event::Text("Gamma".into()));
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(section_names(&h), ["Alpha", "Gamma"]);
    assert!(h.state().sorter.renaming.is_none());

    // Past the double-click window, so the next two clicks count as a new double-click.
    h.run_steps(25);
    let gamma = h.get_by_label("Gamma").rect().center();
    double_click(&mut h, gamma);
    assert!(h.state().sorter.renaming.is_some());
    h.event(egui::Event::Text("Nope".into()));
    h.run_steps(1);
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert_eq!(section_names(&h), ["Alpha", "Gamma"]);
    assert!(h.query_by_label("Nope").is_none());

    h.run_steps(25);
    let gamma = h.get_by_label("Gamma").rect().center();
    double_click(&mut h, gamma);
    assert!(h.state().sorter.renaming.is_some());
    h.event(egui::Event::Text("Draft".into()));
    h.run_steps(1);
    h.state_mut().run("file.new", json!({})).unwrap();
    h.run_steps(2);
    assert!(h.state().sorter.renaming.is_none(), "a different presentation drops the draft");
    h.state_mut().session.set_active(0);
    h.run_steps(2);
    assert_eq!(section_names(&h), ["Alpha", "Gamma"]);
}

#[test]
fn dragging_a_section_moves_it_and_keeps_it_collapsed() {
    let mut h = harness(true);
    let before = section_slides(&h);
    h.get_by_label("Collapse Beta").click();
    h.run_steps(2);
    let (alpha, beta) = (h.get_by_label("Alpha").rect(), h.get_by_label("Beta").rect());
    assert!(beta.top() > alpha.bottom());
    drag(&mut h, beta.center(), Pos2::new(alpha.center().x, alpha.top() - 4.0));
    assert_eq!(section_names(&h), ["Beta", "Alpha"]);
    let after = section_slides(&h);
    assert_eq!(after[0], before[1]);
    assert_eq!(after[1], before[0]);
    let doc = &h.state().session.active().unwrap().doc;
    let order: Vec<u32> = doc.slides.iter().map(|s| s.id.0).collect();
    let expect: Vec<u32> = after.iter().flatten().copied().collect();
    assert_eq!(order, expect, "slides follow the section order");
    assert!(h.query_by_label("Expand Beta").is_some(), "Beta stays collapsed after the move");
    assert!(h.query_by_label("Slide 1").is_none() && h.query_by_label("Slide 2").is_none());
    assert!(h.query_by_label("Slide 3").is_some() && h.query_by_label("Slide 4").is_some());
    let (beta, alpha) = (h.get_by_label("Beta").rect(), h.get_by_label("Alpha").rect());
    assert!(beta.top() < alpha.top());
}

fn slide_order(h: &Harness<'_, SlideApp>) -> Vec<u32> {
    h.state().session.active().unwrap().doc.slides.iter().map(|s| s.id.0).collect()
}

fn selected_slides(h: &Harness<'_, SlideApp>) -> Vec<u32> {
    h.state().session.active().unwrap().selection.slides.iter().map(|s| s.0).collect()
}

#[test]
fn dragging_a_selected_slide_moves_the_whole_selection() {
    let mut h = harness(false);
    let ids = slide_order(&h);
    h.state_mut().run("slide.selectSlides", json!({"ids": [ids[0], ids[2]]})).unwrap();
    h.run_steps(2);
    let (first, last) = (h.get_by_label("Slide 1").rect(), h.get_by_label("Slide 4").rect());
    // The right half of the last slide: the gap after it.
    drag(&mut h, first.center(), Pos2::new(last.right() - 10.0, last.center().y));
    assert_eq!(slide_order(&h), [ids[1], ids[3], ids[0], ids[2]]);
    assert_eq!(selected_slides(&h), [ids[0], ids[2]], "the moved slides stay selected");
    assert!(h.state().thumb_drag.is_none());
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(slide_order(&h), ids, "one undo puts both back");
}

#[test]
fn dragging_an_unselected_slide_moves_only_it() {
    let mut h = harness(false);
    let ids = slide_order(&h);
    h.state_mut().run("slide.selectSlides", json!({"ids": [ids[0], ids[2]]})).unwrap();
    h.run_steps(2);
    let (second, last) = (h.get_by_label("Slide 2").rect(), h.get_by_label("Slide 4").rect());
    drag(&mut h, second.center(), Pos2::new(last.right() - 10.0, last.center().y));
    assert_eq!(slide_order(&h), [ids[0], ids[2], ids[3], ids[1]]);
    assert_eq!(selected_slides(&h), [ids[1]]);
}

#[test]
fn dropping_a_selection_where_it_is_changes_nothing() {
    let mut h = harness(false);
    let ids = slide_order(&h);
    h.state_mut().run("slide.selectSlides", json!({"ids": [ids[1], ids[2]]})).unwrap();
    h.run_steps(2);
    let steps = h.state().session.active().unwrap().history.undo.len();
    let (second, third) = (h.get_by_label("Slide 2").rect(), h.get_by_label("Slide 3").rect());
    drag(&mut h, second.center(), Pos2::new(third.right() - 10.0, third.center().y));
    assert_eq!(slide_order(&h), ids);
    assert_eq!(h.state().session.active().unwrap().history.undo.len(), steps, "a drop in place adds no undo step");
}

#[test]
fn ctrl_a_selects_every_slide() {
    let mut h = harness(false);
    h.key_combination_modifiers(egui::Modifiers::COMMAND, &[egui::Key::A]);
    h.run_steps(2);
    let st = h.state().session.active().unwrap();
    let all = st.doc.slides.len();
    assert_eq!(st.selection.slides.len(), all, "Ctrl+A in the sorter selects every slide");
}

fn selected(h: &Harness<'_, SlideApp>) -> Vec<usize> {
    let st = h.state().session.active().unwrap();
    let mut v: Vec<usize> = st.selection.slides.iter().filter_map(|id| st.doc.slide_index(*id)).collect();
    v.sort_unstable();
    v
}

#[test]
fn shift_and_ctrl_shift_clicks_select_ranges() {
    use egui::Modifiers;
    let mut h = harness(false);
    let click = |h: &mut Harness<'_, SlideApp>, n: usize, m: Modifiers| {
        h.get_by_label(&format!("Slide {n}")).click_modifiers(m);
        h.run_steps(2);
    };
    // Click slide 2, Shift+click slide 4: slides 2–4, slide 4 current.
    click(&mut h, 2, Modifiers::NONE);
    click(&mut h, 4, Modifiers::SHIFT);
    assert_eq!(selected(&h), vec![1, 2, 3]);
    assert_eq!(h.state().session.active().unwrap().selection.slide, 3);
    // A second Shift+click extends from slide 2 again, not from slide 4.
    click(&mut h, 1, Modifiers::SHIFT);
    assert_eq!(selected(&h), vec![0, 1]);
    // Click 1, Ctrl+click 2, Ctrl+Shift+click 4: the range joins what is selected.
    click(&mut h, 1, Modifiers::NONE);
    click(&mut h, 3, Modifiers::COMMAND);
    click(&mut h, 4, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(selected(&h), vec![0, 2, 3]);
}
