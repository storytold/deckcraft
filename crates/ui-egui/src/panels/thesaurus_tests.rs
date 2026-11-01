//! Review › Thesaurus driven like a person: caret in a word, open the pane, click a synonym.

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

#[test]
fn thesaurus_pane_swaps_the_word_at_the_caret() {
    let _gpu = crate::gpu_lock::gpu_lock();
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let id = s.execute("insert.textBox", &json!({"rect": [100, 100, 500, 50]})).unwrap()["id"].as_u64().unwrap();
    s.execute("text.edit", &json!({"id": id})).unwrap();
    s.execute("text.insert", &json!({"text": "Our Goal is growth."})).unwrap();
    s.execute("text.edit", &json!({"id": id, "at": [0, 6]})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.tab = "Review".into();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.get_by_label("Thesaurus").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.pane.as_deref(), Some("thesaurus"));
    assert_eq!(h.state().thesaurus_query, "Goal");
    if let Ok(img) = h.render() {
        let _ = img.save("/tmp/thesaurus.png");
    }
    h.get_by_label("Objective").click();
    h.run_steps(3);
    let text = h
        .state()
        .session
        .active()
        .unwrap()
        .shapes()
        .iter()
        .find(|x| x.id.0 as u64 == id)
        .and_then(|x| x.text.as_ref())
        .map(|t| t.paragraphs[0].text());
    assert_eq!(text.as_deref(), Some("Our Objective is growth."));
    assert_eq!(h.state().thesaurus_query, "Objective", "the pane follows the new word");

    // Typing a word that isn't listed says so instead of showing nothing.
    h.state_mut().thesaurus_query = "zzxq".into();
    h.run_steps(2);
    assert!(h.query_by_label_contains("No synonyms").is_some());
}
