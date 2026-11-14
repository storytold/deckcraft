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
mod effect_presets;
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
mod effect_presets_tests;

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
        // The Fill section's slider comes before the Line section's.
        h.query_all_by_role_and_label(egui::accesskit::Role::Slider, "Transparency").next().unwrap().click();
        h.run_steps(2);
        assert_eq!(solid_alpha(h.state(), b), Some(50_000));
        let changed: Vec<f64> = slider_values(&h);
        assert!(changed.iter().any(|v| (*v - 50.0).abs() < 0.5), "the changed slider shows 50%, got {changed:?}");
    }

    /// Format Shape ▸ Line ▸ Transparency shows a solid line's transparency and sets it in one
    /// undo step; No line has no Transparency row.
    #[test]
    fn line_transparency_slider_shows_and_sets_the_lines_transparency() {
        let mut h = fill_pane(Some(json!({"none": true})));
        let line_alpha = |h: &Harness<'_, SlideApp>| {
            let doc = h.state().session.active().unwrap();
            match doc.selected_shapes()[0].line.as_ref().and_then(|l| l.fill.as_ref()) {
                Some(Fill::Solid { color }) => color.mods.iter().find_map(|m| match m {
                    ColorTransform::Alpha(v) => Some(*v),
                    _ => None,
                }),
                other => panic!("expected a solid line, got {other:?}"),
            }
        };
        h.state_mut().session.execute("shape.line", &json!({"color": "#ff0000", "transparency": 0.4})).unwrap();
        h.run_steps(3);
        let rows = || h.query_all_by_role_and_label(egui::accesskit::Role::Slider, "Transparency").count();
        assert_eq!(rows(), 1, "no fill, so the one Transparency row is the line's");
        let shown = slider_values(&h);
        assert!(shown.iter().any(|v| (*v - 40.0).abs() < 0.5), "a 40% transparent line should show 40%, got {shown:?}");

        h.get_by_role_and_label(egui::accesskit::Role::Slider, "Transparency").click();
        h.run_steps(2);
        assert_eq!(line_alpha(&h), Some(50_000));
        h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
        h.run_steps(2);
        assert_eq!(line_alpha(&h), Some(60_000));

        h.state_mut().session.execute("shape.line", &json!({"none": true})).unwrap();
        h.run_steps(3);
        assert_eq!(h.query_all_by_role_and_label(egui::accesskit::Role::Slider, "Transparency").count(), 0, "No line");
    }

    /// A Transparency slider changed from the keyboard holds its value until it loses the focus.
    /// When an undo or a command run outside the pane changes the shape first, the slider shows
    /// the new value, and leaving it does not write the held one back.
    #[test]
    fn transparency_sliders_drop_a_held_value_when_the_document_changes() {
        let alpha = |fill: Option<&Fill>| match fill {
            Some(Fill::Solid { color }) => color.mods.iter().find_map(|m| match m {
                ColorTransform::Alpha(v) => Some(*v),
                _ => None,
            }),
            other => panic!("expected a solid fill, got {other:?}"),
        };
        for (slider, cmd) in [(0, "shape.fill"), (1, "shape.line")] {
            let mut h = fill_pane(Some(json!({"color": "#00ff00"})));
            h.state_mut().session.execute("shape.line", &json!({"color": "#ff0000"})).unwrap();
            h.state_mut().session.execute(cmd, &json!({"transparency": 0.4})).unwrap();
            h.run_steps(3);
            let read = |h: &Harness<'_, SlideApp>| {
                let doc = h.state().session.active().unwrap();
                let sh = doc.selected_shapes()[0];
                if slider == 0 { alpha(sh.fill.as_ref()) } else { alpha(sh.line.as_ref().and_then(|l| l.fill.as_ref())) }
            };
            let value = |h: &Harness<'_, SlideApp>| {
                h.query_all_by_role_and_label(egui::accesskit::Role::Slider, "Transparency")
                    .nth(slider)
                    .unwrap()
                    .accesskit_node()
                    .numeric_value()
                    .unwrap()
            };
            h.query_all_by_role_and_label(egui::accesskit::Role::Slider, "Transparency").nth(slider).unwrap().focus();
            h.run_steps(2);
            h.key_press(egui::Key::ArrowRight);
            h.run_steps(2);
            assert!(value(&h) > 40.5, "{cmd}: the arrow key moves the slider, got {}", value(&h));
            assert_eq!(read(&h), Some(60_000), "{cmd}: nothing is written while the slider keeps the focus");

            h.state_mut().session.execute(cmd, &json!({"transparency": 0.1})).unwrap();
            h.run_steps(3);
            assert!((value(&h) - 10.0).abs() < 0.5, "{cmd}: the slider shows the new 10%, got {}", value(&h));
            h.key_press(egui::Key::Tab);
            h.run_steps(3);
            assert_eq!(read(&h), Some(90_000), "{cmd}: leaving the slider keeps the 10% the command set");
        }
    }

    /// An effect slider that has the focus holds its value; a command run outside the pane
    /// replaces it, and leaving the slider does not write the held value back.
    #[test]
    fn effect_sliders_drop_a_held_value_when_the_document_changes() {
        let mut h = fill_pane(None);
        h.state_mut().session.execute("shape.effects", &json!({"shadow": "outer"})).unwrap();
        h.state_mut().session.execute("shape.effects", &json!({"shadowProps": {"blur": 4.0}})).unwrap();
        h.state_mut().ui.format_tab = "effects".into();
        h.run_steps(3);
        let blur = |h: &Harness<'_, SlideApp>| {
            let doc = h.state().session.active().unwrap();
            doc.selected_shapes()[0].effects.as_ref().and_then(|e| e.outer_shadow.as_ref()).map(|s| s.blur)
        };
        let value =
            |h: &Harness<'_, SlideApp>| h.get_by_role_and_label(egui::accesskit::Role::Slider, "Blur").accesskit_node().numeric_value().unwrap();
        h.get_by_role_and_label(egui::accesskit::Role::Slider, "Blur").focus();
        h.run_steps(2);
        assert!((value(&h) - 4.0).abs() < 0.5, "the slider shows 4 pt, got {}", value(&h));

        h.state_mut().session.execute("shape.effects", &json!({"shadowProps": {"blur": 10.0}})).unwrap();
        h.run_steps(3);
        assert!((value(&h) - 10.0).abs() < 0.5, "the slider shows the new 10 pt, got {}", value(&h));
        h.key_press(egui::Key::Tab);
        h.run_steps(3);
        assert_eq!(blur(&h), Some(10.0), "leaving the slider keeps the 10 pt the command set");
    }

    /// An automatic line shows the transparency it inherits from the theme's line style.
    #[test]
    fn line_transparency_slider_shows_an_inherited_lines_transparency() {
        let mut h = fill_pane(Some(json!({"none": true})));
        h.state_mut()
            .session
            .edit(|doc, _| {
                for l in &mut std::sync::Arc::make_mut(&mut doc.masters[0]).theme.format.lines {
                    l.fill = Some(Fill::solid(ColorRef::rgb(deckcraft_color::Rgba::rgb(0, 0, 255)).with(ColorTransform::Alpha(50_000))));
                }
                Ok(())
            })
            .unwrap();
        h.run_steps(3);
        assert!(h.state().session.active().unwrap().selected_shapes()[0].line.is_none(), "the line is automatic");
        let shown = slider_values(&h);
        assert!(shown.iter().any(|v| (*v - 50.0).abs() < 0.5), "a line inheriting 50% should show 50%, got {shown:?}");

        // A line's own `alphaMod` shows as the transparency it draws with.
        h.state_mut()
            .session
            .edit(|doc, sel| {
                let id = sel.shapes[0];
                let sh = std::sync::Arc::make_mut(&mut doc.slides[sel.slide]).shape_mut(id).unwrap();
                let color = ColorRef::scheme(SchemeSlot::Accent1).with(ColorTransform::AlphaMod(30_000));
                sh.line = Some(deckcraft_model::Line { fill: Some(Fill::solid(color)), ..Default::default() });
                Ok(())
            })
            .unwrap();
        h.run_steps(3);
        let shown = slider_values(&h);
        assert!(shown.iter().any(|v| (*v - 70.0).abs() < 0.5), "alphaMod 30% should show 70%, got {shown:?}");

        // A `phClr` line shows the alpha of the style color it takes.
        h.state_mut()
            .session
            .edit(|doc, sel| {
                let id = sel.shapes[0];
                let sh = std::sync::Arc::make_mut(&mut doc.slides[sel.slide]).shape_mut(id).unwrap();
                sh.line = Some(deckcraft_model::Line { fill: Some(Fill::solid(ColorRef::scheme(SchemeSlot::PhClr))), ..Default::default() });
                sh.style.as_mut().unwrap().line_ref.1 = ColorRef::rgb(deckcraft_color::Rgba::rgb(0, 0, 255)).with(ColorTransform::Alpha(25_000));
                Ok(())
            })
            .unwrap();
        h.run_steps(3);
        let shown = slider_values(&h);
        assert!(shown.iter().any(|v| (*v - 75.0).abs() < 0.5), "a phClr line on a 25% opaque style color should show 75%, got {shown:?}");
    }

    /// Format Shape ▸ Line ▸ Gradient line gives the line a gradient and shows the gradient editor,
    /// whose edits change the line.
    #[test]
    fn gradient_line_option_sets_and_edits_the_lines_gradient() {
        use egui::accesskit::{Role, Toggled};
        let mut h = fill_pane(Some(json!({"none": true})));
        let line_stops = |h: &Harness<'_, SlideApp>| {
            let doc = h.state().session.active().unwrap();
            match doc.selected_shapes()[0].line.as_ref().and_then(|l| l.fill.as_ref()) {
                Some(Fill::Gradient(g)) => g.stops.len(),
                other => panic!("expected a gradient line, got {other:?}"),
            }
        };
        // The Line section's radio buttons come after the Fill section's, which has its own Automatic.
        let chosen = |h: &Harness<'_, SlideApp>, label: &str| {
            h.query_all_by_role_and_label(Role::RadioButton, label).last().unwrap().accesskit_node().toggled() == Some(Toggled::True)
        };
        let bars = |h: &Harness<'_, SlideApp>| h.query_all_by_label("Gradient bar").count();
        assert!(chosen(&h, "Automatic"));
        assert!(!chosen(&h, "Gradient line"));
        assert_eq!(bars(&h), 0, "no fill and an automatic line, so no gradient editor");

        h.get_by_role_and_label(Role::RadioButton, "Gradient line").click();
        h.run_steps(3);
        assert_eq!(line_stops(&h), 2);
        assert!(chosen(&h, "Gradient line"));
        assert!(!chosen(&h, "Solid line"));
        assert_eq!(bars(&h), 1, "the line's gradient editor");
        assert_eq!(color_buttons(&h), 0, "a gradient line takes its colors from its stops");

        // An edit in the editor changes the line, in one undo step, and keeps the gradient's tile
        // rectangle and scaling.
        let three = json!({"stops": [[0, "accent1"], [0.5, "accent2"], [1, "accent3"]], "angle": 90, "scaled": true, "tile": [0.0, -0.5, 0.0, 0.5]});
        h.state_mut().session.execute("shape.line", &json!({"gradient": three})).unwrap();
        h.run_steps(3);
        assert_eq!(line_stops(&h), 3);
        h.get_by_role_and_label(Role::Button, "Remove stop").click();
        h.run_steps(3);
        assert_eq!(line_stops(&h), 2);
        {
            let doc = h.state().session.active().unwrap();
            let Some(Fill::Gradient(g)) = doc.selected_shapes()[0].line.as_ref().and_then(|l| l.fill.as_ref()) else {
                panic!("expected a gradient line")
            };
            assert_eq!(g.tile, [0.0, -0.5, 0.0, 0.5]);
            assert_eq!(g.shape, deckcraft_model::style::GradientShape::Linear { angle: 90.0, scaled: true });
        }
        // A new direction keeps them too.
        h.get_by_label("→").click();
        h.run_steps(3);
        {
            let doc = h.state().session.active().unwrap();
            let Some(Fill::Gradient(g)) = doc.selected_shapes()[0].line.as_ref().and_then(|l| l.fill.as_ref()) else {
                panic!("expected a gradient line")
            };
            assert_eq!(g.tile, [0.0, -0.5, 0.0, 0.5]);
            assert_eq!(g.shape, deckcraft_model::style::GradientShape::Linear { angle: 0.0, scaled: true });
        }
        h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
        h.run_steps(2);
        h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
        h.run_steps(2);
        assert_eq!(line_stops(&h), 3);

        h.get_by_role_and_label(Role::RadioButton, "Solid line").click();
        h.run_steps(3);
        assert!(chosen(&h, "Solid line"));
        assert_eq!(bars(&h), 0);
        assert_eq!(color_buttons(&h), 1, "the solid line's Color button");
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
            // Each count includes the Line section's row for the rectangle's automatic line.
            ("automatic", None, true, 2),
            ("solid", Some(json!({"color": "accent2"})), true, 2),
            ("no fill", Some(json!({"none": true})), false, 1),
            // The other Transparency row is the selected gradient stop's.
            ("gradient", Some(gradient), false, 2),
            ("picture", Some(picture_fill(0.0)), false, 2),
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

    /// Slide background fill is one of the Fill options. It has no Color button or Transparency
    /// slider of its own.
    #[test]
    fn slide_background_fill_is_a_fill_option() {
        let mut h = fill_pane(None);
        let fill = |h: &Harness<'_, SlideApp>| h.state().session.active().unwrap().selected_shapes()[0].fill.clone();
        let undo = h.state().session.active().unwrap().history.undo.len();
        h.get_by_label("Slide background fill").click();
        h.run_steps(3);
        assert_eq!(fill(&h), Some(Fill::Background));
        assert_eq!(h.state().session.active().unwrap().history.undo.len(), undo + 1, "one undo step");
        assert_eq!(h.get_by_label("Slide background fill").accesskit_node().toggled(), Some(egui::accesskit::Toggled::True));
        assert_eq!(color_buttons(&h), 1, "only the line's Color button");
        assert_eq!(h.query_all_by_role_and_label(egui::accesskit::Role::Slider, "Transparency").count(), 1, "only the line's Transparency slider");
        // A shape that already has the background fill shows this option chosen, not Automatic.
        let h = fill_pane(Some(json!({"background": true})));
        assert_eq!(h.get_by_label("Slide background fill").accesskit_node().toggled(), Some(egui::accesskit::Toggled::True));
        let automatic = h.query_all_by_label("Automatic").filter(|n| n.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)).count();
        assert_eq!(automatic, 1, "only the line's Automatic is chosen");
    }

    /// A picture cannot save a slide background fill, so its Fill options leave it out. A group
    /// that holds a shape keeps the option.
    #[test]
    fn a_picture_has_no_slide_background_fill_option() {
        let pane = |group: bool| {
            let mut s = Session::with_new();
            s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
            let png = deckcraft_render::Image { width: 2, height: 2, pixels: [255, 0, 0, 255].repeat(4) }.to_png();
            let pic =
                s.execute("insert.picture", &json!({"data": deckcraft_engine::cmd::base64_encode(&png), "name": "red.png"})).unwrap()["id"].clone();
            if group {
                let shape = s.execute("shape.insert", &json!({"preset": "rect", "rect": [500.0, 40.0, 120.0, 80.0]})).unwrap()["id"].clone();
                s.execute("edit.select", &json!({"ids": [pic, shape]})).unwrap();
                s.execute("arrange.group", &json!({})).unwrap();
                s.execute("shape.fill", &json!({"color": "#ff0000"})).unwrap();
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
        };
        let h = pane(false);
        assert!(h.query_by_label("Pattern fill").is_some(), "the Fill options are shown");
        assert!(h.query_by_label("Slide background fill").is_none());
        let mut h = pane(true);
        h.get_by_label("Slide background fill").click();
        h.run_steps(3);
        let st = h.state().session.active().unwrap();
        let group = st.selected_shapes()[0];
        let fills: Vec<_> = group.children().iter().map(|m| m.fill.clone()).collect();
        assert_eq!(fills[1], Some(Fill::Background), "the group's shape takes the fill");
        assert!(matches!(fills[0], Some(Fill::Solid { .. })), "the picture keeps its fill: {:?}", fills[0]);
        assert_eq!(group.fill, None, "the group's own fill is cleared");
        assert_eq!(h.get_by_label("Slide background fill").accesskit_node().toggled(), Some(egui::accesskit::Toggled::True));
        assert_eq!(h.query_all_by_role_and_label(egui::accesskit::Role::Slider, "Transparency").count(), 0);
        assert_eq!(color_buttons(&h), 1, "only the line's Color button");
    }

    /// A group whose picture uses the group's fill keeps that fill, and still shows Slide
    /// background fill chosen once its shapes have it.
    #[test]
    fn a_group_that_keeps_its_fill_shows_slide_background_fill() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        let png = deckcraft_render::Image { width: 2, height: 2, pixels: [0, 0, 0, 0].repeat(4) }.to_png();
        let pic = s.execute("insert.picture", &json!({"data": deckcraft_engine::cmd::base64_encode(&png), "name": "clear.png"})).unwrap()["id"]
            .as_u64()
            .unwrap();
        let shape = s.execute("shape.insert", &json!({"preset": "rect", "rect": [500.0, 40.0, 120.0, 80.0]})).unwrap()["id"].as_u64().unwrap();
        s.execute("edit.select", &json!({"ids": [pic, shape]})).unwrap();
        s.execute("arrange.group", &json!({})).unwrap();
        s.execute("shape.fill", &json!({"gradient": {"stops": [[0, "#ff0000"], [1, "#0000ff"]]}})).unwrap();
        s.edit(|doc, _| {
            for slide in &mut doc.slides {
                let slide = std::sync::Arc::make_mut(slide);
                if let Some(p) = slide.shape_mut(ShapeId(pic as u32)) {
                    p.fill = Some(Fill::Group);
                }
            }
            Ok(())
        })
        .unwrap();
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
        h.get_by_label("Slide background fill").click();
        h.run_steps(3);
        let st = h.state().session.active().unwrap();
        assert!(matches!(st.selected_shapes()[0].fill, Some(Fill::Gradient(_))), "the group keeps the fill its picture uses");
        assert_eq!(st.shape(ShapeId(shape as u32)).unwrap().fill, Some(Fill::Background));
        assert_eq!(h.get_by_label("Slide background fill").accesskit_node().toggled(), Some(egui::accesskit::Toggled::True));
        assert!(h.query_by_label("Rotate with shape").is_none(), "no editor for the group's kept gradient");
        assert_eq!(h.query_all_by_role_and_label(egui::accesskit::Role::Slider, "Transparency").count(), 0);
        assert_eq!(color_buttons(&h), 1, "only the line's Color button");
    }
}

#[cfg(test)]
mod thesaurus_tests;
