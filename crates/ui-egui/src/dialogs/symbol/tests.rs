use super::*;
use deckcraft_engine::Session;
use egui_kittest::{Harness, kittest::Queryable};

fn harness() -> Harness<'static, SlideApp> {
    let mut session = Session::with_new();
    session.execute("insert.textBox", &json!({"text": ""})).unwrap();
    let mut app = SlideApp::new(session, crate::Services::default());
    app.run("app.dialog", json!({"id": "symbol"})).unwrap();
    let mut h = Harness::builder().with_size(vec2(1000.0, 700.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(12);
    h
}

fn form<'a>(h: &'a Harness<'_, SlideApp>) -> &'a SymbolForm {
    h.state().dialog.as_ref().unwrap().symbol.as_ref().unwrap()
}

fn text(app: &SlideApp) -> String {
    let state = app.session.doc().unwrap();
    let selection = state.selection.text.as_ref().unwrap();
    deckcraft_engine::cmd::text::body_of(state, selection).unwrap().text()
}

fn code(h: &mut Harness<'_, SlideApp>, value: &str) {
    h.get_by_label("Character code").click();
    h.run_steps(1);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.get_by_label("Character code").type_text(value);
    h.run_steps(2);
}

#[test]
fn font_picker_code_insertion_recent_symbols_and_close() {
    let mut h = harness();
    assert!(form(&h).chars.len() > 189, "font coverage replaces the old fixed list");
    h.get_by_label("Font").click();
    h.run_steps(2);
    h.get_by_label("Search fonts").click();
    h.run_steps(1);
    h.get_by_label("Search fonts").type_text("Liberation Serif");
    h.run_steps(2);
    h.get_by_label("Liberation Serif").click();
    h.run_steps(3);
    assert_eq!(form(&h).font, "Liberation Serif");
    code(&mut h, "03B1");
    assert_eq!(form(&h).selected, Some('α'));
    h.get_by_role_and_label(egui::Role::Window, "Symbol").get_by_label("Insert").click_accesskit();
    h.run_steps(3);
    assert_eq!(text(h.state()), "α");
    assert_eq!(h.state().ui.recent_symbols, vec![RecentSymbol { text: "α".into(), font: "Liberation Serif".into() }]);
    h.get_by_role_and_label(egui::Role::Window, "Symbol").get_by_label("Insert").click_accesskit();
    h.run_steps(2);
    assert_eq!(text(h.state()), "αα");
    assert_eq!(h.state().ui.recent_symbols.len(), 1);
    h.get_by_label("Close").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    h.state_mut().session.execute("text.insert", &json!({"text": "z"})).unwrap();
    let state = h.state().session.doc().unwrap();
    let body = deckcraft_engine::cmd::text::body_of(state, state.selection.text.as_ref().unwrap()).unwrap();
    assert_ne!(deckcraft_model::edit::props_at(body, (0, 3)).font.as_deref(), Some("Liberation Serif"));
}

#[test]
fn invalid_and_unsupported_codes_cannot_insert_the_previous_selection() {
    let mut h = harness();
    for value in ["D800", "110000", "0000", "XYZ"] {
        code(&mut h, value);
        assert_eq!(form(&h).selected, None, "{value}");
        h.get_by_role_and_label(egui::Role::Window, "Symbol").get_by_label("Insert").click_accesskit();
        h.run_steps(1);
        assert_eq!(text(h.state()), "");
    }
}

#[test]
fn subset_jump_and_grid_keyboard_navigation() {
    let mut h = harness();
    h.get_by_label("Subset").click();
    h.run_steps(2);
    h.get_by_label("Currency Symbols").scroll_to_me();
    h.run_steps(2);
    h.get_by_label("Currency Symbols").click_accesskit();
    h.run_steps(3);
    let selected = form(&h).selected.unwrap();
    assert!((0x20A0..=0x20CF).contains(&(selected as u32)));
    h.get_by_label(&format!("U+{:04X}", selected as u32)).click();
    h.run_steps(2);
    assert_eq!(form(&h).selected, Some(selected));
    let index = form(&h).chars.binary_search(&selected).unwrap();
    h.key_press(egui::Key::ArrowRight);
    h.run_steps(2);
    h.key_press(egui::Key::ArrowRight);
    h.run_steps(2);
    assert_eq!(form(&h).selected, form(&h).chars.get(index + 2).copied());
}

#[test]
fn special_characters_insert_without_requiring_a_font_subset() {
    let mut h = harness();
    h.get_by_label("Special Characters").click();
    h.run_steps(3);
    h.get_by_label("Trademark").click();
    h.run_steps(2);
    h.get_by_role_and_label(egui::Role::Window, "Symbol").get_by_label("Insert").click_accesskit();
    h.run_steps(2);
    assert_eq!(text(h.state()), "™");
    assert!(h.state().ui.recent_symbols[0].font.is_empty());
}

#[test]
fn character_codes_handle_supplementary_planes_and_reject_missing_glyphs() {
    let mut f = SymbolForm { chars: vec!['©', '😀'], ..Default::default() };
    for (code, expected) in [("U+1F600", Some('😀')), ("00A9", Some('©')), ("0041", None), ("D800", None)] {
        f.code = code.into();
        f.select_code();
        assert_eq!(f.selected, expected);
    }
}

#[test]
fn glyph_previews_use_the_selected_font_and_have_transparent_backgrounds() {
    let inter = glyph_image("Inter", 'A', 1.0);
    let serif = glyph_image("Liberation Serif", 'A', 1.0);
    assert!(inter.pixels.iter().any(|p| p.a() != 0));
    assert!(inter.pixels.iter().any(|p| p.a() == 0));
    if FontDb::global().has_family("Inter") && FontDb::global().has_family("Liberation Serif") {
        assert_ne!(inter.pixels, serif.pixels);
    }
}

#[test]
fn recent_symbols_are_bounded_sanitized_and_persisted() {
    let mut ui =
        crate::UiState { recent_symbols: (0..30).map(|_| RecentSymbol { text: "©".into(), font: "Inter".into() }).collect(), ..Default::default() };
    ui.recent_symbols.push(RecentSymbol { text: "\n".into(), font: "Inter".into() });
    ui.recent_symbols.push(RecentSymbol { text: "™".into(), font: "x".repeat(257) });
    let clean = ui.sanitized();
    assert_eq!(clean.recent_symbols.len(), 1);
    let encoded = serde_json::to_string(&clean).unwrap();
    assert_eq!(serde_json::from_str::<crate::UiState>(&encoded).unwrap().recent_symbols, clean.recent_symbols);
    let clean = crate::UiState {
        recent_symbols: (33..63).filter_map(char::from_u32).map(|c| RecentSymbol { text: c.to_string(), font: String::new() }).collect(),
        ..Default::default()
    }
    .sanitized();
    assert_eq!(clean.recent_symbols.len(), RECENT_MAX);
}
