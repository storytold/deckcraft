//! The widget on a context it shares with a host: it must not depend on, or change, the
//! host's fonts and style. WordCraft's dialog used `FontFamily::Name("semibold")`, which panics
//! on any context that never registered that family; the widget must work with and without it.

use egui::{FontFamily, FontId, Key, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use equashion_egui::EquationEditor;

const SEMIBOLD: &str = "semibold";

struct Host {
    editor: EquationEditor,
    /// Install fonts with a `semibold` family in the first frame (as WordCraft's theme does).
    register: bool,
    frames: u32,
    dark: bool,
}

/// Both themes' styles and the active theme (what `set_style` / `set_visuals` would change).
fn styles(ctx: &egui::Context) -> (egui::Style, egui::Style, egui::Theme) {
    ((*ctx.style_of(egui::Theme::Dark)).clone(), (*ctx.style_of(egui::Theme::Light)).clone(), ctx.theme())
}

fn has_semibold(ctx: &egui::Context) -> bool {
    ctx.fonts(|f| f.definitions().families.contains_key(&FontFamily::Name(SEMIBOLD.into())))
}

fn harness(register: bool, dark: bool) -> Harness<'static, Host> {
    Harness::builder().with_size(vec2(820.0, 760.0)).build_ui_state(
        |ui, h: &mut Host| {
            h.frames += 1;
            if h.frames == 1 {
                ui.ctx().set_theme(if h.dark { egui::Theme::Dark } else { egui::Theme::Light });
                if h.register {
                    let mut defs = egui::FontDefinitions::default();
                    let first = defs.families.get(&FontFamily::Proportional).and_then(|v| v.first().cloned()).unwrap_or_default();
                    defs.families.insert(FontFamily::Name(SEMIBOLD.into()), vec![first]);
                    ui.ctx().set_fonts(defs);
                }
                return;
            }
            if h.register {
                // The host's own text in its named family keeps working next to the widget.
                ui.label(egui::RichText::new("Host heading").font(FontId::new(14.0, FontFamily::Name(SEMIBOLD.into()))));
            }
            h.editor.show(ui);
        },
        Host { editor: EquationEditor::new(), register, frames: 0, dark },
    )
}

/// Every part of the widget: all palette tabs, typing, a palette insert, the linear field,
/// the discard prompt.
fn exercise(h: &mut Harness<'static, Host>) {
    h.run();
    h.run();
    h.run();
    for tab in ["Greek", "Operators", "Structures"] {
        h.get_by_label(tab).click();
        h.run();
    }
    h.event(egui::Event::Text("x^2".into()));
    h.run();
    h.get_by_label("a/b").click_accesskit();
    h.run();
    h.event(egui::Event::Text("\\nabla\\oint".into()));
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert!(h.state().editor.message().is_some(), "the discard prompt is drawn");
    h.get_by_role(egui::accesskit::Role::TextInput).click();
    h.run();
    h.event(egui::Event::Text("+1".into()));
    h.run();
    assert!(h.state().editor.linear().contains('1'), "{}", h.state().editor.linear());
}

#[test]
fn works_on_a_context_that_never_registered_a_named_family() {
    let mut h = harness(false, false);
    h.run();
    assert!(!has_semibold(&h.ctx));
    let style = styles(&h.ctx);
    let fonts = h.ctx.fonts(|f| f.definitions().clone());
    exercise(&mut h);
    assert!(!has_semibold(&h.ctx), "the widget registered no font family");
    assert_eq!(styles(&h.ctx), style, "the widget did not change the style or visuals");
    assert!(h.ctx.fonts(|f| f.definitions() == &fonts), "the widget did not change the fonts");
}

#[test]
fn works_on_a_context_where_the_host_registered_a_named_family() {
    let mut h = harness(true, true);
    h.run();
    h.run();
    assert!(has_semibold(&h.ctx));
    let style = styles(&h.ctx);
    let fonts = h.ctx.fonts(|f| f.definitions().clone());
    exercise(&mut h);
    assert!(has_semibold(&h.ctx), "the host's family is still there");
    assert_eq!(styles(&h.ctx), style, "the widget did not change the style or visuals");
    assert!(h.ctx.fonts(|f| f.definitions() == &fonts), "the widget did not change the fonts");
    h.get_by_label("Host heading");
}

/// The control: what the test above guards against does panic. A widget that asks for a named
/// family the host never registered (as WordCraft's `theme::semibold` did) takes the app down.
#[test]
#[should_panic(expected = "not bound")]
fn a_named_family_the_host_never_registered_panics() {
    let mut h = Harness::builder().with_size(vec2(400.0, 200.0)).build_ui(|ui| {
        ui.label(egui::RichText::new("Fractions").font(FontId::new(11.0, FontFamily::Name(SEMIBOLD.into()))));
    });
    h.run();
}
