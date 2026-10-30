//! Task panes on the right: Format Shape, Format Background, Animation, Selection, Comments and
//! Design Ideas.

use deckcraft_model::{AnimClass, AnimStart, Fill, ShapeKind};
use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::icons::{self, Icon};
use crate::ribbon::{cref_param, paint_anim_tile};
use crate::theme::{self, Tokens};
use crate::{SlideApp, widgets};

mod animation;
mod background;
mod comments;
mod designer;
mod format_shape;
mod smartart;
mod thesaurus;

use animation::animation;
use background::background;
use comments::{comments, revisions};
use designer::designer;
use format_shape::format_shape;
pub(crate) use format_shape::text_transparency_slider;
#[cfg(test)]
use format_shape::{fill_transparency_to_apply, solid_fill_transparency_percent};
use smartart::smartart;
use thesaurus::thesaurus;

pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(pane) = app.ui.pane.clone() else { return };
    egui::Panel::right("task_pane")
        .resizable(true)
        .default_size(app.ui.pane_width)
        .size_range(240.0..=520.0)
        .frame(egui::Frame::NONE.fill(t.card).inner_margin(egui::Margin::same(10)))
        .show(ui, |ui| {
            app.ui.pane_width = ui.available_width() + 20.0;
            let title = match pane.as_str() {
                "format" => "Format Shape",
                "background" => "Format Background",
                "animation" => "Animation Pane",
                "selection" => "Selection",
                "comments" => "Comments",
                "designer" => "Design Ideas",
                "smartart" => "Text Pane",
                "thesaurus" => "Thesaurus",
                "revisions" => "Revisions",
                _ => "Pane",
            };
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(title).font(theme::bold(16.0)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(r, CornerRadius::same(4), t.hover);
                    }
                    icons::paint(ui.painter(), r.shrink(3.0), Icon::Close, t.text_dim, false);
                    if resp.clicked() {
                        app.ui.pane = None;
                    }
                });
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match pane.as_str() {
                "format" => format_shape(app, ui),
                "background" => background(app, ui),
                "animation" => animation(app, ui),
                "selection" => crate::selpane::show(app, ui),
                "comments" => comments(app, ui),
                "designer" => designer(app, ui),
                "smartart" => smartart(app, ui),
                "thesaurus" => thesaurus(app, ui),
                "revisions" => revisions(app, ui),
                _ => {}
            });
        });
}

fn run(app: &mut SlideApp, id: &str, p: Value) {
    let _ = app.run(id, p);
}

fn scheme(app: &SlideApp) -> deckcraft_color::ColorScheme {
    app.session.active().and_then(|d| d.doc.masters.first().map(|m| m.scheme())).unwrap_or_else(|| deckcraft_model::Theme::default().colors)
}

fn color_button(
    app: &mut SlideApp,
    ui: &mut Ui,
    label: &str,
    none_label: Option<&str>,
    on_pick: impl Fn(Option<deckcraft_model::ColorRef>) -> (String, Value),
) {
    let sc = scheme(app);
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let r = widgets::drop_button(ui, "Color", vec2(70.0, 22.0), true);
            widgets::color_popup(&r).show(|ui| {
                if let Some(c) = widgets::color_grid(ui, &sc, none_label) {
                    let (id, p) = on_pick(c);
                    run(app, &id, p);
                }
            });
        });
    });
}

pub mod animtimeline;
pub mod fillui;
pub mod notes;
pub mod selpane;
#[cfg(test)]
mod smartart_tests;
pub mod thumbs;

#[cfg(test)]
mod background_tests;

#[cfg(test)]
mod pattern_fill_tests;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use deckcraft_color::{ColorTransform, SchemeSlot};
    use deckcraft_engine::Session;
    use deckcraft_model::{ColorRef, Fill, ShapeId};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use serde_json::json;

    use super::{fill_transparency_to_apply, solid_fill_transparency_percent};
    use crate::{Services, SlideApp};

    #[test]
    fn dash_types_set_the_dash_and_the_cap_that_shows_them() {
        use super::format_shape::dash_params;
        assert_eq!(dash_params("roundDot"), json!({"dash": "dot", "cap": "round"}));
        assert_eq!(dash_params("dot"), json!({"dash": "dot", "cap": "flat"}));
        assert_eq!(dash_params("lgDash"), json!({"dash": "lgDash"}));
    }

    #[test]
    fn solid_fill_transparency_reads_the_alpha_shape_fill_writes() {
        assert_eq!(solid_fill_transparency_percent(None), 0.0);
        assert_eq!(solid_fill_transparency_percent(Some(&Fill::None)), 0.0);
        assert_eq!(solid_fill_transparency_percent(Some(&Fill::Group)), 0.0);
        let plain = Fill::solid(ColorRef::scheme(SchemeSlot::Accent1));
        assert_eq!(solid_fill_transparency_percent(Some(&plain)), 0.0);
        // `shape.fill` transparency 0.4 stores Alpha((1 - 0.4) * 100_000).
        let forty = Fill::solid(ColorRef::scheme(SchemeSlot::Accent1).with(ColorTransform::Alpha(60_000)));
        let p = solid_fill_transparency_percent(Some(&forty));
        assert!((p - 40.0).abs() < 0.01, "{p}");
        let clear = Fill::solid(ColorRef::scheme(SchemeSlot::Accent1).with(ColorTransform::Alpha(0)));
        assert!((solid_fill_transparency_percent(Some(&clear)) - 100.0).abs() < 0.01);
        // A later alpha wins, matching ColorRef::alpha.
        let layered = Fill::solid(ColorRef::scheme(SchemeSlot::Accent1).with(ColorTransform::Alpha(0)).with(ColorTransform::Alpha(60_000)));
        assert!((solid_fill_transparency_percent(Some(&layered)) - 40.0).abs() < 0.01);

        assert_eq!(fill_transparency_to_apply(40.0, 40.0), None);
        assert_eq!(fill_transparency_to_apply(40.005, 40.0), None);
        assert_eq!(fill_transparency_to_apply(f32::NAN, 40.0), None);
        assert_eq!(fill_transparency_to_apply(55.0, 40.0), Some(0.55));
        assert_eq!(fill_transparency_to_apply(0.0, 40.0), Some(0.0));
    }

    fn solid_alpha(app: &SlideApp, id: u32) -> Option<i32> {
        let sh = app.session.active()?.shape(ShapeId(id))?;
        let Fill::Solid { color } = sh.fill.as_ref()? else { return None };
        color.mods.iter().find_map(|m| match m {
            ColorTransform::Alpha(v) => Some(*v),
            _ => None,
        })
    }

    fn slider_values(h: &Harness<'_, SlideApp>) -> Vec<f64> {
        h.query_all_by_role(egui::accesskit::Role::Slider).filter_map(|n| n.accesskit_node().numeric_value()).collect()
    }

    /// The Fill slider shows the selected solid's transparency, and opening the pane
    /// (or looking at another shape first) does not write 0% over it.
    #[test]
    fn fill_transparency_slider_follows_the_selection() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        let a = s.execute("shape.insert", &json!({"preset": "rect", "rect": [40.0, 40.0, 120.0, 80.0]})).unwrap()["id"].as_u64().unwrap() as u32;
        let b = s.execute("shape.insert", &json!({"preset": "rect", "rect": [200.0, 40.0, 120.0, 80.0]})).unwrap()["id"].as_u64().unwrap() as u32;
        s.execute("shape.fill", &json!({"id": a, "color": "accent1", "transparency": 0.0})).unwrap();
        s.execute("shape.fill", &json!({"id": b, "color": "accent1", "transparency": 0.4})).unwrap();
        s.execute("edit.select", &json!({"ids": [a]})).unwrap();
        let mut app = SlideApp::new(s, Services::default());
        app.ui.pane = Some("format".into());
        app.ui.format_tab = "fill".into();
        let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(3);
        let opaque: Vec<f64> = slider_values(&h);
        assert!(opaque.iter().any(|v| (*v - 0.0).abs() < 0.5), "opaque shape should show 0%, got {opaque:?}");
        assert!(opaque.iter().all(|v| (*v - 40.0).abs() > 0.5), "opaque shape should not show 40%, got {opaque:?}");

        h.state_mut().session.execute("edit.select", &json!({"ids": [b]})).unwrap();
        h.run_steps(3);
        let clear: Vec<f64> = slider_values(&h);
        assert!(clear.iter().any(|v| (*v - 40.0).abs() < 0.5), "40% fill should show 40%, got {clear:?}");
        assert_eq!(solid_alpha(h.state(), b), Some(60_000));

        // 0.37 exposes the slider's numeric entry as a SpinButton with the same name; target the
        // Slider itself so the click lands on the slider, not its text field. A click at its
        // middle is a real edit (50%), not a side effect of opening the pane or selecting b.
        h.get_by_role_and_label(egui::accesskit::Role::Slider, "Transparency").click();
        h.run_steps(2);
        assert_eq!(solid_alpha(h.state(), b), Some(50_000));
        let changed: Vec<f64> = slider_values(&h);
        assert!(changed.iter().any(|v| (*v - 50.0).abs() < 0.5), "the changed slider shows 50%, got {changed:?}");
    }

    /// A blank slide with one selected rectangle, its fill set by `fill` (none for an automatic fill),
    /// in Format Shape ▸ Fill.
    fn fill_pane(fill: Option<serde_json::Value>) -> Harness<'static, SlideApp> {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [40.0, 40.0, 120.0, 80.0]})).unwrap()["id"].as_u64().unwrap();
        s.execute("edit.select", &json!({"ids": [id]})).unwrap();
        if let Some(p) = fill {
            s.execute("shape.fill", &p).unwrap();
        }
        let mut app = SlideApp::new(s, Services::default());
        app.ui.pane = Some("format".into());
        app.ui.format_tab = "fill".into();
        let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(3);
        h
    }

    fn picture_fill(transparency: f64) -> serde_json::Value {
        let png = deckcraft_render::Image { width: 2, height: 2, pixels: [255, 0, 0, 255].repeat(4) }.to_png();
        json!({"picture": deckcraft_engine::cmd::base64_encode(&png), "transparency": transparency})
    }

    /// The "Color" drop buttons in the pane: the fill's, when it shows, and the line's.
    fn color_buttons(h: &Harness<'_, SlideApp>) -> usize {
        h.query_all_by_role(egui::accesskit::Role::Button).filter(|n| n.accesskit_node().label().is_some_and(|l| l.trim() == "Color")).count()
    }

    /// Fill colour and transparency controls appear only for fill kinds they can actually modify.
    #[test]
    fn fill_color_and_transparency_show_only_where_they_change_the_fill() {
        let gradient = json!({"gradient": {"stops": [[0, "accent1"], [1, "accent2"]], "angle": 90}});
        for (name, fill, color, transparency) in [
            ("automatic", None, true, 1),
            ("solid", Some(json!({"color": "accent2"})), true, 1),
            ("no fill", Some(json!({"none": true})), false, 0),
            // The one Transparency row left is the selected gradient stop's.
            ("gradient", Some(gradient), false, 1),
            ("picture", Some(picture_fill(0.0)), false, 1),
        ] {
            let h = fill_pane(fill);
            let fill_color = color_buttons(&h) == 2;
            assert_eq!(fill_color, color, "{name}: fill Color button");
            // 0.37 gives each slider's numeric entry its own SpinButton with the same name, so count
            // the sliders themselves: one per visible Transparency row.
            assert_eq!(
                h.query_all_by_role_and_label(egui::accesskit::Role::Slider, "Transparency").count(),
                transparency,
                "{name}: Transparency rows"
            );
        }
    }

    /// The picture-fill transparency slider reflects and edits the picture fill's own transparency.
    #[test]
    fn picture_fill_transparency_slider_shows_the_picture_transparency() {
        let mut h = fill_pane(Some(picture_fill(0.4)));
        let alpha = |h: &Harness<'_, SlideApp>| {
            let doc = h.state().session.active().unwrap();
            match &doc.selected_shapes()[0].fill {
                Some(Fill::Picture(pf)) => pf.alpha,
                other => panic!("expected a picture fill, got {other:?}"),
            }
        };
        let shown = slider_values(&h);
        assert!(shown.iter().any(|v| (*v - 40.0).abs() < 0.5), "a 40% transparent picture should show 40%, got {shown:?}");
        // 0.37 exposes the slider's numeric entry as a SpinButton with the same name and excludes
        // a label that names another control from label queries, so find the painted label itself:
        // clicking it must leave the picture's transparency alone.
        let label = h
            .get_all_by_role(egui::accesskit::Role::Label)
            .find(|n| n.value().as_deref() == Some("Transparency"))
            .expect("the Transparency label is there");
        label.click();
        h.run_steps(2);
        assert!(alpha(&h).is_some_and(|a| (a - 0.6).abs() < 1e-9), "{:?}", alpha(&h));
    }
}

#[cfg(test)]
mod thesaurus_tests;
