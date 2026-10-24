//! Home › Replace Fonts…: the dialog lists the deck's fonts and replaces one, as one undo step.

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

fn harness() -> Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("insert.textBox", &json!({"rect": [40, 40, 400, 80], "text": "Hello fonts"})).unwrap();
    s.execute("text.exit", &json!({})).unwrap();
    s.execute("format.font", &json!({"family": "Comic Neue"})).unwrap();
    s.execute("format.size", &json!({"size": 40})).unwrap();
    let app = SlideApp::new(s, Services::default());
    // Home's ribbon is wider than it was when this test was written: at 1400 the Editing group
    // begins at x=1398, so the Replace Fonts button is two pixels wide on screen and a click at
    // its centre lands outside the window. The ribbon does not scroll sideways, so the window has
    // to be wide enough to hold it -- 1800 is what the other full-ribbon tests here use.
    let mut h = Harness::builder().with_size(egui::vec2(1800.0, 1100.0)).with_step_dt(1.0 / 60.0).build_ui_state(
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

#[test]
fn ribbon_button_opens_the_dialog_and_replaces() {
    let mut h = harness();
    h.get_by_label("Replace Fonts…").click();
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
    let mut h = harness();
    h.render().unwrap().save("/tmp/replacefonts0.png").unwrap();
    h.state_mut().dialog = Some(crate::dialogs::Dialog::new("replaceFonts"));
    h.run_steps(4);
    h.render().unwrap().save("/tmp/replacefonts.png").unwrap();
}
