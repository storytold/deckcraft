//! Review › Compare: the Revisions pane lists a revised copy's changes and accepts them.

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

/// The sample deck compared with a copy whose slide 2 is gone and that has a new last slide.
fn harness() -> (Harness<'static, SlideApp>, usize) {
    let mut s = Session::new();
    deckcraft_engine::sample::open_sample(&mut s).unwrap();
    let n = s.doc().unwrap().doc.slides.len();
    let data = s.execute("file.saveBytes", &json!({})).unwrap()["data"].as_str().unwrap().to_string();
    s.execute("file.openBytes", &json!({"name": "rev.deckcraft", "data": data})).unwrap();
    s.execute("slide.delete", &json!({"index": 1})).unwrap();
    s.execute("slide.go", &json!({"index": n - 2})).unwrap();
    s.execute("slide.new", &json!({"layout": "titleAndContent", "title": "From the reviewer", "body": "x"})).unwrap();
    let rev = s.execute("file.saveBytes", &json!({})).unwrap()["data"].as_str().unwrap().to_string();
    s.close_document(1);
    s.set_active(0);
    s.execute("review.compare", &json!({"name": "Q3 review.deckcraft", "data": rev})).unwrap();
    let app = SlideApp::new(s, Services::default());
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    (h, n)
}

#[test]
fn pane_opens_lists_changes_and_accept_all_applies_them() {
    let (mut h, n) = harness();
    assert_eq!(h.state().ui.pane.as_deref(), Some("revisions"), "compare opens the Revisions pane");
    assert!(h.query_by_label("Compared with Q3 review.deckcraft").is_some());
    assert!(h.query_by_label("Slide 2 deleted").is_some());
    assert!(h.query_by_label_contains("Slide inserted after").is_some());
    h.get_by_label_contains("Accept All").click();
    h.run_steps(3);
    let st = h.state().session.active().unwrap();
    assert_eq!(st.doc.slides.len(), n, "one slide gone, one slide added");
    assert!(h.query_by_label("No differences left.").is_some());
    h.get_by_label("End Review").click();
    h.run_steps(3);
    assert!(h.state().session.active().unwrap().compare.is_none());
}

#[test]
fn reject_all_empties_the_pane_and_keeps_the_deck() {
    let (mut h, n) = harness();
    let doc = h.state().session.active().unwrap().doc.clone();
    h.get_by_label_contains("Reject All").click();
    h.run_steps(3);
    let st = h.state().session.active().unwrap();
    assert!(std::sync::Arc::ptr_eq(&doc, &st.doc), "nothing applied");
    assert_eq!(st.doc.slides.len(), n);
    assert!(h.query_by_label("No differences left.").is_some());
}

#[test]
#[ignore = "writes a screenshot for review"]
fn screenshot() {
    let _gpu = crate::gpu_lock::gpu_lock();
    let (mut h, _) = harness();
    // Review tab.
    h.state_mut().ui.tab = "Review".into();
    h.run_steps(4);
    h.render().unwrap().save("/tmp/compare.png").unwrap();
}
