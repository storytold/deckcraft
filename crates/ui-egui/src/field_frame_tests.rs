//! Text fields must show where they are: in the light theme the field was as white as the dialog,
//! pane or ribbon it sits on and had no outline, so only its label (or hint) was visible (#1204).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use egui::accesskit::Role;
use egui::epaint::Shape;
use egui::{Color32, Rect, Stroke};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp, theme};

fn harness(app: SlideApp) -> Harness<'static, SlideApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    // The app keeps asking for frames, so step instead of run().
    h.run_steps(3);
    h
}

/// The largest per-channel difference between two colours.
fn distance(a: Color32, b: Color32) -> u8 {
    a.r().abs_diff(b.r()).max(a.g().abs_diff(b.g())).max(a.b().abs_diff(b.b()))
}

/// The frame egui painted for the text field at `field` (its fill and outline).
fn frame_at(h: &Harness<'_, SlideApp>, field: Rect) -> (Color32, Stroke) {
    h.output()
        .shapes
        .iter()
        .find_map(|c| match &c.shape {
            Shape::Rect(r) if r.rect.min.distance(field.min) < 2.0 && r.rect.max.distance(field.max) < 2.0 => Some((r.fill, r.stroke)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no frame painted for the field at {field:?}"))
}

/// What `c` shows when painted over the opaque `surface`. `Color32` is premultiplied, so each
/// channel is `c + surface * (255 - alpha) / 255`.
fn over(c: Color32, surface: Color32) -> Color32 {
    let behind = |s: u8| (u16::from(s) * u16::from(255 - c.a()) / 255) as u8;
    Color32::from_rgb(c.r().saturating_add(behind(surface.r())), c.g().saturating_add(behind(surface.g())), c.b().saturating_add(behind(surface.b())))
}

/// A frame with this `fill` and `stroke` stands apart from the opaque `surface`: it paints a fill
/// or an outline (at least 1 px wide) that, as actually shown over the surface, differs from it.
fn stands_apart(fill: Color32, stroke: Stroke, surface: Color32) -> bool {
    let outlined = stroke.width >= 1.0 && distance(over(stroke.color, surface), surface) >= 24;
    outlined || distance(over(fill, surface), surface) >= 16
}

/// The field stands apart from `surface` by its fill or by its outline.
fn assert_visible(h: &Harness<'_, SlideApp>, what: &str, surface: Color32) {
    let fields: Vec<Rect> = h.query_all_by_role(Role::TextInput).map(|n| n.rect()).collect();
    assert!(!fields.is_empty(), "{what}: no text field");
    for field in fields {
        let (fill, stroke) = frame_at(h, field);
        assert!(
            stands_apart(fill, stroke, surface),
            "{what}: the field at {field:?} (fill {fill:?}, outline {stroke:?}) can't be told from the {surface:?} behind it"
        );
    }
}

#[test]
fn only_a_painted_fill_or_outline_stands_apart() {
    let surface = Color32::WHITE;
    // Stroke::NONE is transparent black: nothing is painted, whatever its RGB.
    assert!(!stands_apart(surface, Stroke::NONE, surface));
    assert!(!stands_apart(Color32::WHITE, Stroke::NONE, Color32::from_rgb(250, 250, 250)));
    // A fully transparent fill shows the surface.
    assert!(!stands_apart(Color32::TRANSPARENT, Stroke::NONE, surface));
    assert!(!stands_apart(Color32::TRANSPARENT, Stroke::new(0.5, Color32::BLACK), surface));
    // A transparent outline paints nothing either.
    assert!(!stands_apart(surface, Stroke::new(1.0, Color32::TRANSPARENT), surface));
    // A real 1 px dark outline, or a clearly different opaque fill, does.
    assert!(stands_apart(surface, Stroke::new(1.0, Color32::from_gray(120)), surface));
    assert!(stands_apart(Color32::from_gray(200), Stroke::NONE, surface));
}

#[test]
fn light_text_fields_show_in_dialogs_panes_and_the_ribbon() {
    let light = theme::Tokens::light();

    // Home › Find: "Find what:" in the dialog card.
    let mut h = harness(SlideApp::new(Session::with_new(), Services::default()));
    h.state_mut().run("app.find", json!({})).unwrap();
    h.run_steps(3);
    assert_visible(&h, "Find and Replace", h.ctx.global_style().visuals.window_fill);
    // Still visible with the keyboard in it.
    let field = h.get_by_role(Role::TextInput).rect();
    h.get_by_role(Role::TextInput).click();
    h.run_steps(2);
    assert!(h.ctx.memory(|m| m.focused().is_some()), "the click puts the keyboard in the field");
    let (fill, stroke) = frame_at(&h, field);
    // Same surface as the dialog check above: the field sits on the dialog's window fill.
    let surface = h.ctx.global_style().visuals.window_fill;
    assert!(stands_apart(fill, stroke, surface), "focused field: fill {fill:?}, outline {stroke:?}");

    // Review › Thesaurus: the search box on the task pane.
    let mut h = harness(SlideApp::new(Session::with_new(), Services::default()));
    h.state_mut().ui.pane = Some("thesaurus".into());
    h.run_steps(3);
    assert_visible(&h, "Thesaurus pane", light.card);

    // Transitions › Duration: the spinner on the ribbon.
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.ui.tab = "Transitions".into();
    let h = harness(app);
    assert_visible(&h, "Transitions ribbon", light.card);
}
