//! Design › Fonts › Customize Fonts… opens Create New Theme Fonts and saves in one step.

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::{Services, SlideApp};

fn harness() -> Harness<'static, SlideApp> {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.ui.tab = "Design".into();
    let mut h = Harness::builder().with_size(egui::vec2(1800.0, 1100.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn scheme(h: &Harness<'_, SlideApp>) -> deckcraft_model::theme::FontScheme {
    h.state().session.active().unwrap().doc.masters[0].theme.fonts.clone()
}

fn undo_len(h: &Harness<'_, SlideApp>) -> usize {
    h.state().session.active().unwrap().history.undo.len()
}

fn open_dialog(h: &mut Harness<'_, SlideApp>) {
    h.get_by_label("Fonts").click();
    h.run_steps(2);
    h.get_by_label("Customize Fonts…").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("themeFonts"));
}

#[test]
fn customize_fonts_opens_from_the_design_tab() {
    let mut h = harness();
    let before = scheme(&h);
    let n = undo_len(&h);
    open_dialog(&mut h);
    assert_eq!(h.get_by_label("Scheme name").value().as_deref(), Some(before.name.as_str()));
    assert_eq!(h.get_by_label("Heading font").value().as_deref(), Some(before.major.latin.as_str()));
    assert_eq!(h.get_by_label("Body font").value().as_deref(), Some(before.minor.latin.as_str()));
    let _ = h.get_by_label(&format!("Heading sample  {}", before.major.latin));
    let _ = h.get_by_label(&format!("Body sample  {}", before.minor.latin));
    let _ = h.get_by_label("Save");
    let _ = h.get_by_label("Cancel");
    let save = h.get_by_label("Save").rect();
    let win = h.get_by_label("Create New Theme Fonts").rect();
    assert!(win.contains(save.center()), "Save {save:?} is outside the dialog window {win:?}");
    h.get_by_label("Save").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    assert_eq!(scheme(&h), before);
    assert_eq!(undo_len(&h), n, "saving the current fonts is not an undo step");
}

#[test]
fn save_applies_the_name_and_both_faces() {
    let mut h = harness();
    let before = scheme(&h);
    let n = undo_len(&h);
    open_dialog(&mut h);
    {
        let d = h.state_mut().dialog.as_mut().unwrap();
        d.fields.insert("schemeName".into(), "Talk".into());
        d.fields.insert("major".into(), "Lato".into());
        d.fields.insert("minor".into(), "Roboto".into());
    }
    h.get_by_label("Save").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none(), "status: {:?}", h.state().status);
    let after = scheme(&h);
    assert_eq!(after.name, "Talk");
    assert_eq!(after.major.latin, "Lato");
    assert_eq!(after.minor.latin, "Roboto");
    assert_eq!(after.major.ea, before.major.ea);
    assert_eq!(undo_len(&h), n + 1);
    assert_eq!(h.state().session.active().unwrap().history.undo.last().unwrap().label, "Fonts");
}

#[test]
fn a_blank_heading_keeps_the_dialog_open() {
    let mut h = harness();
    let before = scheme(&h);
    let n = undo_len(&h);
    open_dialog(&mut h);
    h.state_mut().dialog.as_mut().unwrap().fields.insert("major".into(), "  ".into());
    h.get_by_label("Save").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("themeFonts"));
    assert_eq!(scheme(&h), before);
    assert_eq!(undo_len(&h), n);
    let status = h.state().status.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
    assert!(status.contains("heading"), "{status}");
}

#[test]
fn cancel_closes_without_editing() {
    let mut h = harness();
    let before = scheme(&h);
    let n = undo_len(&h);
    open_dialog(&mut h);
    h.state_mut().dialog.as_mut().unwrap().fields.insert("major".into(), "Lato".into());
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    assert_eq!(scheme(&h), before);
    assert_eq!(undo_len(&h), n);
}

#[test]
fn typing_a_name_replaces_the_scheme_name() {
    let mut h = harness();
    let n = undo_len(&h);
    open_dialog(&mut h);
    h.get_by_label("Scheme name").click();
    h.run_steps(1);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.get_by_label("Scheme name").type_text("Talk");
    h.run_steps(1);
    let shown = h.get_by_label("Scheme name").value().unwrap_or_default();
    assert_eq!(shown, "Talk", "the name field should replace its text, got {shown}");
    h.get_by_label("Save").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none(), "status: {:?}", h.state().status);
    assert_eq!(scheme(&h).name, "Talk");
    assert_eq!(scheme(&h).major.latin, "Inter");
    assert_eq!(scheme(&h).minor.latin, "Inter");
    assert_eq!(undo_len(&h), n + 1);
}
