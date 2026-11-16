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

#[test]
fn extra_colours_are_listed_added_and_removed() {
    use deckcraft_color::Rgba;
    let mut h = harness();
    let two = vec!["#C7EBBA", "#0D4A21"];
    h.state_mut()
        .session
        .execute(
            "design.colors",
            &serde_json::json!({"computedTints": false, "tints": {"accent5": two, "accent6": []}, "extra": [{"name": "Sand", "color": "#EBE5E0", "tints": two}, {"name": "Navy", "color": "#000061"}]}),
        )
        .unwrap();
    h.run_steps(2);
    let n = undo_len(&h);
    open_dialog(&mut h);
    assert_eq!(h.get_by_label("Extra colour 1 name").value().as_deref(), Some("Sand"));
    assert_eq!(h.get_by_label("Extra colour 2 hex").value().as_deref(), Some("#000061"));
    let save = h.get_by_label("Save").rect();
    let win = h.get_by_label("Create New Theme Colors").rect();
    assert!(win.contains(save.center()), "Save {save:?} is outside the dialog window {win:?}");
    // Remove Navy, add Digital blue.
    h.get_by_label("Remove extra colour 2").click();
    h.run_steps(2);
    h.get_by_label("Add colour").click();
    h.run_steps(2);
    {
        let d = h.state_mut().dialog.as_mut().unwrap();
        assert_eq!(d.fields["extraCount"], "2");
        d.fields.insert("extra.1.name".into(), "Digital blue".into());
        d.fields.insert("extra.1.color".into(), "#0029ED".into());
    }
    h.get_by_label("Save").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none(), "status: {:?}", h.state().status);
    let after = scheme(&h);
    assert_eq!(after.extra.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), vec!["Sand", "Digital blue"]);
    assert_eq!(after.extra[1].color, Rgba::rgb(0x00, 0x29, 0xED));
    assert_eq!(after.extra[0].tints.as_ref().map(Vec::len), Some(2), "Sand keeps its tint rows");
    assert_eq!(after.tints.get(&SchemeSlot::Accent5).map(Vec::len), Some(2), "slot tint rows pass through");
    assert_eq!(after.tints.get(&SchemeSlot::Accent6).map(Vec::len), Some(0), "an empty list passes through");
    assert!(!after.computed_tints, "the checkbox keeps the flag");
    assert_eq!(undo_len(&h), n + 1);
    // The checkbox turns computed rows back on.
    open_dialog(&mut h);
    h.get_by_label("Computed tints and shades under colours without their own rows").click();
    h.run_steps(2);
    h.get_by_label("Save").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none(), "status: {:?}", h.state().status);
    assert!(scheme(&h).computed_tints);
    assert_eq!(undo_len(&h), n + 2);
    // An extra colour without a name keeps the dialog open.
    open_dialog(&mut h);
    h.get_by_label("Add colour").click();
    h.run_steps(2);
    h.get_by_label("Save").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("themeColors"));
    let status = h.state().status.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
    assert!(status.contains("needs a name"), "{status}");
}

/// An extra colour's explicit tint rows follow its row through a rename, and through the removal
/// of a row above it; a row added in the dialog has none.
#[test]
fn renaming_or_removing_extra_colours_keeps_the_others_tint_rows() {
    use serde_json::json;
    let sand = vec!["#C7EBBA", "#0D4A21"];
    let navy = vec!["#000061"];
    let extras = json!([{"name": "Sand", "color": "#EBE5E0", "tints": sand}, {"name": "Navy", "color": "#000061", "tints": navy}]);
    let mut h = harness();
    h.state_mut().session.execute("design.colors", &json!({"extra": extras})).unwrap();
    h.run_steps(2);
    open_dialog(&mut h);
    h.state_mut().dialog.as_mut().unwrap().fields.insert("extra.0.name".into(), "Beach".into());
    h.get_by_label("Add colour").click();
    h.run_steps(2);
    {
        let d = h.state_mut().dialog.as_mut().unwrap();
        d.fields.insert("extra.2.name".into(), "Mint".into());
        d.fields.insert("extra.2.color".into(), "#A0FFC0".into());
    }
    h.get_by_label("Save").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none(), "status: {:?}", h.state().status);
    let extra = scheme(&h).extra;
    let rows = |e: &deckcraft_color::ExtraColor| e.tints.as_ref().map(|t| t.iter().map(|c| format!("#{}", c.hex())).collect::<Vec<_>>());
    assert_eq!(extra.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Beach", "Navy", "Mint"]);
    assert_eq!(rows(&extra[0]).as_deref(), Some(&["#C7EBBA".to_string(), "#0D4A21".to_string()][..]), "renamed: rows kept");
    assert_eq!(rows(&extra[1]).as_deref(), Some(&["#000061".to_string()][..]));
    assert_eq!(rows(&extra[2]), None, "an added colour has no rows");
    // Remove the first row: the second keeps its own rows, not the first's.
    open_dialog(&mut h);
    h.get_by_label("Remove extra colour 1").click();
    h.run_steps(2);
    h.get_by_label("Save").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none(), "status: {:?}", h.state().status);
    let extra = scheme(&h).extra;
    assert_eq!(extra.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Navy", "Mint"]);
    assert_eq!(rows(&extra[0]).as_deref(), Some(&["#000061".to_string()][..]), "removal above: rows kept");
    assert_eq!(rows(&extra[1]), None);
}
