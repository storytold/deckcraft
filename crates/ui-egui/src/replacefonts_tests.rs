//! Home › Replace Fonts…: the dialog lists the deck's fonts and replaces one, as one undo step.

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

/// Build the standard Replace Fonts UI harness with a fresh session.
fn harness() -> Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("insert.textBox", &json!({"rect": [40, 40, 400, 80], "text": "Hello fonts"})).unwrap();
    s.execute("text.exit", &json!({})).unwrap();
    s.execute("format.font", &json!({"family": "Comic Neue"})).unwrap();
    s.execute("format.size", &json!({"size": 40})).unwrap();
    let app = SlideApp::new(s, Services::default());
    // The product's default window. The Home ribbon overflows here with the bundled interface
    // fonts; the ribbon's own edge chevron must make its Editing group reachable instead of the
    // regression widening the fixture past the width the app actually opens at.
    let mut h = Harness::builder().with_size(egui::vec2(1440.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn run_font(h: &Harness<'_, SlideApp>) -> Option<String> {
    let st = h.state().session.active().unwrap();
    st.current_slide()
        .unwrap()
        .shapes
        .iter()
        .find_map(|s| s.text.as_ref())
        .and_then(|b| b.paragraphs.first())
        .and_then(|p| p.runs.first())
        .and_then(|r| r.props.font.clone())
}

/// Scroll the ribbon to its right edge through its own visible overflow control.
fn reveal_ribbon_end(h: &mut Harness<'_, SlideApp>) {
    for _ in 0..16 {
        let Some(cue) = h.query_by_label("Scroll ribbon right") else { break };
        cue.click();
        h.run_steps(1);
    }
}

/// The ribbon command opens Replace Fonts; its Replace button updates the document and leaves
/// the dialog open with a confirmation.
#[test]
fn ribbon_button_opens_the_dialog_and_replaces() {
    let mut h = harness();
    reveal_ribbon_end(&mut h);
    let button = h.get_by_label("Replace Fonts…");
    assert!(button.rect().max.x <= 1440.0 && button.rect().min.x >= 0.0, "Replace Fonts… is on screen at the default width: {:?}", button.rect());
    if let Some(left) = h.query_by_label("Scroll ribbon left") {
        let cue = left.rect();
        assert!(!button.rect().intersects(cue), "the left overflow cue {cue:?} does not cover Replace Fonts… {:?}", button.rect());
        assert!(button.rect().min.x >= cue.max.x, "ribbon content starts after the reserved left chevron gutter");
    }
    button.click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("replaceFonts"));
    // Pick the font to replace and type the new one, the way a person would.
    if let Some(d) = h.state_mut().dialog.as_mut() {
        d.fields.insert("from".into(), "Comic Neue".into());
        d.fields.insert("to".into(), "Lato".into());
    }
    h.run_steps(2);
    // The ribbon has a Replace button too (Find and Replace); the dialog's is drawn last.
    h.query_all_by_label("Replace").last().expect("the dialog's Replace button").click();
    h.run_steps(3);
    assert_eq!(run_font(&h).as_deref(), Some("Lato"));
    // The dialog stays open with a confirmation, like PowerPoint's.
    assert!(h.query_by_label_contains("Replaced “Comic Neue”").is_some());
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(run_font(&h).as_deref(), Some("Comic Neue"));
}

#[test]
#[ignore = "writes a screenshot for review"]
fn screenshot() {
    let _gpu = crate::gpu_lock::gpu_lock();
    let mut h = harness();
    h.render().unwrap().save("/tmp/replacefonts0.png").unwrap();
    h.state_mut().dialog = Some(crate::dialogs::Dialog::new("replaceFonts"));
    h.run_steps(4);
    h.render().unwrap().save("/tmp/replacefonts.png").unwrap();
}
