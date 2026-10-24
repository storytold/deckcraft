//! The Preferences (Options) dialog: controls apply as they change and Cancel, Esc and the
//! window's close button put the settings back the way they were (#752, reported as #878).

use deckcraft_engine::Session;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::{Harness, Node};
use serde_json::json;

use crate::theme::Brightness;
use crate::{Services, SlideApp};

/// The slider itself, not the label drawn beside it.
fn interface_size<'a>(h: &'a Harness<'_, SlideApp>) -> Node<'a> {
    h.get_all_by_label("Interface size").find(|n| n.accesskit_node().role() == egui::accesskit::Role::Slider).expect("the slider is there")
}

fn harness() -> Harness<'static, SlideApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    // The app keeps asking for frames, so step instead of run().
    h.run_steps(3);
    h
}

fn open_options(h: &mut Harness<'_, SlideApp>) {
    h.state_mut().run("app.preferences", json!({})).expect("open Options");
    h.run_steps(3);
}

fn dark(app: &SlideApp) -> bool {
    app.ui.brightness == Brightness::Dark
}

/// A checkbox applies as it is toggled and Cancel puts the old value back.
#[test]
fn options_cancel_undoes_the_changes() {
    let mut h = harness();
    let was = h.state().session.prefs.smart_guides;
    open_options(&mut h);

    h.query_by_label("Show smart guides").expect("the checkbox is there").click();
    h.run_steps(2);
    assert_eq!(h.state().session.prefs.smart_guides, !was, "the change applies as it is made");

    h.query_by_label("Cancel").expect("the Cancel button is there").click();
    h.run_steps(2);
    assert_eq!(h.state().session.prefs.smart_guides, was, "Cancel restores the setting");
    assert!(h.state().dialog.is_none(), "and the dialog closes");
}

/// Every Editing setting and the name field go back on Cancel, not only the first checkbox: a
/// reporter who changed a different control and saw Cancel do nothing has the same answer.
#[test]
fn options_cancel_undoes_every_editing_setting() {
    let mut h = harness();
    let before = h.state().session.prefs.clone();
    open_options(&mut h);

    for label in ["Snap objects to grid", "Replace straight quotes with smart quotes", "AutoCorrect as you type"] {
        h.query_by_label(label).unwrap_or_else(|| panic!("the {label} checkbox is there")).click();
        h.run_steps(1);
    }
    // The name field: it writes into the settings as it is typed.
    // The name field is the last text input of the dialog (the label is a separate widget).
    let name = h.get_all_by_role(egui::accesskit::Role::TextInput).last().expect("the name field is there");
    name.click();
    h.run_steps(1);
    h.event(egui::Event::Text("Z".into()));
    h.run_steps(1);
    let changed = h.state().session.prefs.clone();
    assert_ne!(
        (changed.snap_to_grid, changed.smart_quotes, changed.autocorrect),
        (before.snap_to_grid, before.smart_quotes, before.autocorrect),
        "the settings apply as they are changed"
    );
    assert_ne!(changed.author, before.author, "the name applies as it is typed: {:?}", changed.author);

    h.query_by_label("Cancel").expect("the Cancel button is there").click();
    h.run_steps(2);
    let after = h.state().session.prefs.clone();
    assert_eq!(after.snap_to_grid, before.snap_to_grid, "snap to grid is restored");
    assert_eq!(after.smart_quotes, before.smart_quotes, "smart quotes are restored");
    assert_eq!(after.autocorrect, before.autocorrect, "autocorrect is restored");
    assert_eq!(after.author, before.author, "the name is restored");
}

/// The appearance toggle and the interface size go back on Cancel, too; Esc acts as Cancel.
#[test]
fn options_cancel_puts_the_appearance_and_scale_back() {
    let mut h = harness();
    let (dark_before, scale_before) = (dark(h.state()), h.state().ui.ui_scale);
    open_options(&mut h);

    h.query_by_label("Dark appearance").expect("the checkbox is there").click();
    h.run_steps(2);
    assert!(dark(h.state()), "the appearance applies as it is toggled");

    // Clicking the slider's middle sets it near the middle of its range, away from the default.
    interface_size(&h).click();
    h.run_steps(2);
    let scale_in_dialog = h.state().ui.ui_scale;
    assert!((scale_in_dialog - scale_before).abs() > 1e-3, "the slider applied a new size");

    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert_eq!(dark(h.state()), dark_before, "Esc restores the appearance");
    assert!((h.state().ui.ui_scale - scale_before).abs() < 1e-3, "Esc restores the size");
    assert!(h.state().dialog.is_none(), "and the dialog closes");
}

/// The window's close button is the same as Cancel: the settings go back.
#[test]
fn options_close_button_restores_the_settings() {
    let mut h = harness();
    let was = h.state().session.prefs.smart_guides;
    open_options(&mut h);

    h.query_by_label("Show smart guides").expect("the checkbox is there").click();
    h.run_steps(2);
    assert_eq!(h.state().session.prefs.smart_guides, !was, "the change applies as it is made");

    h.get_all_by_label("Close window").next().expect("the window's close button is there").click();
    h.run_steps(2);
    assert_eq!(h.state().session.prefs.smart_guides, was, "the close button restores the setting");
    assert!(h.state().dialog.is_none(), "and the dialog closes");
}
