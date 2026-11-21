//! Home ▸ Font: a family with sizes or widths lists them in a submenu that a click (or a tap)
//! opens as well as hovering; the font in use is marked; each design is named in full.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use egui::accesskit::Toggled;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use super::home::font_menu;

struct State {
    current: String,
    picked: Option<String>,
}

fn harness(current: &str) -> Harness<'static, State> {
    let fams: Vec<String> = ["Arial", "Bricolage Grotesque", "Bricolage Grotesque 24pt", "Bricolage Grotesque Condensed", "Inter"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    Harness::builder().with_size(egui::vec2(800.0, 600.0)).build_ui_state(
        move |ui, st: &mut State| {
            ui.menu_button("Font", |ui| {
                if let Some(f) = font_menu(ui, &fams, 0, &st.current) {
                    st.picked = Some(f);
                }
            });
        },
        State { current: current.into(), picked: None },
    )
}

#[test]
fn a_click_opens_a_familys_designs_and_picks_one() {
    let mut h = harness("Arial");
    h.get_by_label("Font").click();
    h.run();
    // Clicked, not hovered: touch and keyboard users reach the designs too.
    h.get_by_label("Bricolage Grotesque").click();
    h.run();
    // The design shows its suffix but is named in full.
    h.get_by_label("Bricolage Grotesque Condensed").click();
    h.run();
    assert_eq!(h.state().picked.as_deref(), Some("Bricolage Grotesque Condensed"));
}

#[test]
fn the_font_in_use_is_marked() {
    let mut h = harness("Bricolage Grotesque 24pt");
    h.get_by_label("Font").click();
    h.run();
    // The entries are selectable: on for the font in use, off for the rest.
    assert_eq!(h.get_by_label("Arial").accesskit_node().toggled(), Some(Toggled::False));
    h.get_by_label("Bricolage Grotesque").click();
    h.run();
    assert_eq!(h.get_by_label("Bricolage Grotesque 24pt").accesskit_node().toggled(), Some(Toggled::True));
    assert_eq!(h.get_by_label("Bricolage Grotesque Condensed").accesskit_node().toggled(), Some(Toggled::False));
}
