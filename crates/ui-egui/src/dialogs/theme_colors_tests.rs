//! Design › Colors › Customize Colors… opens Create New Theme Colors and saves in one step.

use deckcraft_color::SchemeSlot;
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

fn scheme(h: &Harness<'_, SlideApp>) -> deckcraft_color::ColorScheme {
    h.state().session.active().unwrap().doc.masters[0].theme.colors.clone()
}

fn undo_len(h: &Harness<'_, SlideApp>) -> usize {
    h.state().session.active().unwrap().history.undo.len()
}

fn open_dialog(h: &mut Harness<'_, SlideApp>) {
    h.get_by_label("Colors").click();
    h.run_steps(2);
    h.get_by_label("Customize Colors…").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("themeColors"));
}

#[test]
fn customize_colors_opens_from_the_design_tab() {
    let mut h = harness();
    let before = scheme(&h);
    let n = undo_len(&h);
    open_dialog(&mut h);
    assert_eq!(h.get_by_label("Scheme name").value().as_deref(), Some(before.name.as_str()));
    let accent = format!("#{}", before.get(SchemeSlot::Accent1).hex());
    assert_eq!(h.get_by_label("Accent 1 hex").value().as_deref(), Some(accent.as_str()));
    let _ = h.get_by_label("Text/Background - Dark 1 hex");
    let _ = h.get_by_label("Text/Background - Light 1 hex");
    let _ = h.get_by_label("Followed Hyperlink hex");
    let _ = h.get_by_label("Hyperlink hex");
    let _ = h.get_by_label("Save");
    let _ = h.get_by_label("Cancel");
    let save = h.get_by_label("Save").rect();
    let win = h.get_by_label("Create New Theme Colors").rect();
    assert!(win.contains(save.center()), "Save {save:?} is outside the dialog window {win:?}");
    h.get_by_label("Save").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    assert_eq!(scheme(&h), before);
    assert_eq!(undo_len(&h), n, "saving the current colours is not an undo step");
}

#[test]
fn save_applies_the_name_and_one_colour() {
    let mut h = harness();
    let before = scheme(&h);
    let n = undo_len(&h);
    open_dialog(&mut h);
    {
        let d = h.state_mut().dialog.as_mut().unwrap();
        d.fields.insert("schemeName".into(), "Talk".into());
        d.fields.insert("accent1".into(), "#ABCDEF".into());
    }
    h.get_by_label("Save").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none(), "status: {:?}", h.state().status);
    let after = scheme(&h);
    assert_eq!(after.name, "Talk");
    assert_eq!(after.get(SchemeSlot::Accent1).hex(), "ABCDEF");
    assert_eq!(after.get(SchemeSlot::Dk1), before.get(SchemeSlot::Dk1));
    assert_eq!(undo_len(&h), n + 1);
    assert_eq!(h.state().session.active().unwrap().history.undo.last().unwrap().label, "Colors");
}

#[test]
fn a_bad_hex_keeps_the_dialog_open() {
    let mut h = harness();
    let before = scheme(&h);
    let n = undo_len(&h);
    open_dialog(&mut h);
    h.state_mut().dialog.as_mut().unwrap().fields.insert("accent1".into(), "zzzzzz".into());
    h.get_by_label("Save").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("themeColors"));
    assert_eq!(scheme(&h), before);
    assert_eq!(undo_len(&h), n);
    let status = h.state().status.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
    assert!(status.contains("Accent 1"), "{status}");
}

#[test]
fn cancel_closes_without_editing() {
    let mut h = harness();
    let before = scheme(&h);
    let n = undo_len(&h);
    open_dialog(&mut h);
    h.state_mut().dialog.as_mut().unwrap().fields.insert("accent1".into(), "#112233".into());
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    assert_eq!(scheme(&h), before);
    assert_eq!(undo_len(&h), n);
}

#[test]
fn typing_a_hex_replaces_accent_1() {
    let mut h = harness();
    let n = undo_len(&h);
    open_dialog(&mut h);
    h.get_by_label("Accent 1 hex").click();
    h.run_steps(1);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.get_by_label("Accent 1 hex").type_text("#11AA22");
    h.run_steps(1);
    let shown = h.get_by_label("Accent 1 hex").value().unwrap_or_default();
    assert_eq!(shown, "#11AA22", "the hex field should replace its text, got {shown}");
    h.get_by_label("Save").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none(), "status: {:?}", h.state().status);
    assert_eq!(scheme(&h).name, "DeckCraft");
    assert_eq!(scheme(&h).get(SchemeSlot::Accent1).hex(), "11AA22");
    assert_eq!(undo_len(&h), n + 1);
}
