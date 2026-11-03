//! Original category-axis controls; round trips rebuild the chart instead of retaining XML.
use std::sync::Arc;

use deckcraft_color::Rgba;
use deckcraft_engine::render::{RenderOpts, render_slide};
use deckcraft_model::chart::{Chart, ChartLayout, ChartTextRole as Role, ChartType, Series};
use deckcraft_model::text::{RunProps, Spacing};
use deckcraft_model::{ColorRef, Fill, Presentation, Shape, ShapeId, ShapeKind, Slide, Xfrm};

fn original() -> Presentation {
    let mut chart = Chart::sample(ChartType::Column);
    chart.title = None;
    chart.legend = None;
    chart.categories = vec!["A\nB".into()];
    chart.series = vec![Series { values: vec![Some(7.)], fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(180, 180, 180)))), ..Default::default() }];
    chart.y_min = Some(0.);
    chart.y_max = Some(10.);
    chart.y_major = Some(2.);
    chart.text_styles.insert(
        Role::Default,
        RunProps { font: Some("Inter".into()), size: Some(20.), fill: Some(Fill::solid(ColorRef::rgb(Rgba::BLACK))), ..Default::default() },
    );
    chart.text_styles.insert(Role::CategoryAxis, RunProps { fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(255, 0, 0)))), ..Default::default() });
    chart.format.plot_layout =
        Some(ChartLayout { inner: true, x: Some(0.1), y: Some(0.15), w: Some(0.7), h: Some(0.65), x_edge: true, y_edge: true, ..Default::default() });
    Presentation {
        slides: vec![Arc::new(Slide {
            shapes: vec![Shape {
                id: ShapeId(1),
                xfrm: Some(Xfrm::new(0., 0., 900., 400.)),
                kind: ShapeKind::Chart(Box::new(chart)),
                ..Default::default()
            }],
            ..Default::default()
        })],
        ..Default::default()
    }
}

fn chart(p: &Presentation) -> &Chart {
    let ShapeKind::Chart(c) = &p.slides[0].shapes[0].kind else { panic!("chart") };
    c
}

fn chart_mut(p: &mut Presentation) -> &mut Chart {
    let ShapeKind::Chart(c) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
    c
}

fn rebuild(p: &Presentation) -> Presentation {
    let mut back = deckcraft_pptx::import(&deckcraft_pptx::export(p).unwrap()).unwrap();
    chart_mut(&mut back).raw = None;
    chart_mut(&mut back).kept = Default::default();
    deckcraft_pptx::import(&deckcraft_pptx::export(&back).unwrap()).unwrap()
}

#[test]
fn explicit_axis_fonts_control_ticks_and_gutters_when_the_chart_default_changes() {
    for fixed_plot in [false, true] {
        let mut images = Vec::new();
        for size in [8., 16., 30., 44.] {
            let mut p = original();
            let c = chart_mut(&mut p);
            c.categories = vec!["A".into()];
            c.y_major = None;
            if !fixed_plot {
                c.format.plot_layout = None;
            }
            c.text_styles.get_mut(&Role::Default).unwrap().size = Some(size);
            c.text_styles.get_mut(&Role::CategoryAxis).unwrap().size = Some(16.);
            c.text_styles.insert(Role::ValueAxis, RunProps { size: Some(16.), ..Default::default() });
            images.push(render_slide(&rebuild(&p), 0, &RenderOpts::default()).pixels);
        }
        // Native controls retain identical plot bounds and axis ink: the explicit
        // axis size governs tick density, geometry, and text placement together.
        assert!(images.windows(2).all(|pair| pair[0] == pair[1]), "fixed plot: {fixed_plot}");
    }
}

#[test]
fn automatic_and_outer_plot_reserves_follow_axis_font_metrics() {
    let cases = [
        ("Inter", 6., 48., 740., 360., false, [100.8425, 34.0375, 729., 325.9625]),
        ("Inter", 6., 48., 740., 360., true, [169.8425, 83.0375, 592., 258.9625]),
        ("Open Sans", 8., 40., 1180., 620., false, [93.7291, 32.2375, 1169., 587.7625]),
        ("Open Sans", 8., 40., 1180., 620., true, [206.7291, 120.2375, 944., 468.7625]),
        ("Lato", 13., 21., 780., 420., false, [50.3683, 17.6, 769., 389.3468]),
        ("Lato", 13., 21., 780., 420., true, [123.3683, 75.6, 624., 310.3468]),
        ("Lato", 27., 9., 1120., 640., false, [25.3016, 11., 1109., 583.3334]),
        ("Lato", 27., 9., 1120., 640., true, [132.3017, 101.4, 896., 460.3334]),
    ];
    for (font, category, value, w, h, outer, expected) in cases {
        let mut p = original();
        p.slide_size = deckcraft_geom::Size::new(1440., 900.);
        Arc::make_mut(&mut p.slides[0]).shapes[0].xfrm = Some(Xfrm::new(0., 0., w, h));
        let c = chart_mut(&mut p);
        c.categories = vec!["A".into()];
        c.plot_fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(0, 255, 0))));
        c.format.plot_layout = outer.then_some(ChartLayout {
            x: Some(0.1),
            y: Some(0.15),
            w: Some(0.7),
            h: Some(0.65),
            x_edge: true,
            y_edge: true,
            ..Default::default()
        });
        for (role, size) in [(Role::CategoryAxis, category), (Role::ValueAxis, value)] {
            c.text_styles.insert(role, RunProps { font: Some(font.into()), size: Some(size), ..Default::default() });
        }
        let bounds = green_bounds(&rebuild(&p));
        // Independent native controls include endpoint-dominated bottom margins,
        // unequal axis fonts, and the absence of outer-frame padding in manual plots.
        for (actual, expected) in bounds.into_iter().zip(expected) {
            assert!((actual - expected).abs() <= 1., "{font}, {category}/{value}, outer={outer}: {bounds:?}");
        }
    }
}

fn green_bounds(p: &Presentation) -> [f64; 4] {
    let img = render_slide(p, 0, &RenderOpts::default());
    let mut bounds = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for (i, px) in img.pixels.chunks_exact(4).enumerate() {
        if px[0] < 20 && px[1] > 240 && px[2] < 20 {
            let (x, y) = ((i as u32 % img.width) as f64, (i as u32 / img.width) as f64);
            bounds = [bounds[0].min(x), bounds[1].min(y), bounds[2].max(x + 1.), bounds[3].max(y + 1.)];
        }
    }
    bounds
}

#[test]
fn overlapping_axis_reserves_keep_a_visible_plot_and_series() {
    for (w, h) in [(960., 40.), (30., 400.)] {
        let mut p = original();
        Arc::make_mut(&mut p.slides[0]).shapes[0].xfrm = Some(Xfrm::new(0., 0., w, h));
        let c = chart_mut(&mut p);
        c.categories = vec!["A".into()];
        c.format.plot_layout = None;
        c.y_major = Some(10.);
        c.text_styles.get_mut(&Role::Default).unwrap().size = Some(16.);
        c.plot_fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(0, 255, 0))));
        c.series[0].fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(0, 0, 255))));
        let back = rebuild(&p);
        let bounds = green_bounds(&back);
        assert!(bounds.iter().all(|v| v.is_finite()) && bounds[2] > bounds[0] && bounds[3] > bounds[1], "{w}x{h}: {bounds:?}");
        let img = render_slide(&back, 0, &RenderOpts::default());
        assert!(img.pixels.chunks_exact(4).any(|px| px[0] < 20 && px[1] < 20 && px[2] > 240), "visible series: {w}x{h}");
    }
}

#[test]
fn category_axis_state_and_spacing_survive_forced_rebuild_and_old_json() {
    for offset in [None, Some(0), Some(50), Some(1000)] {
        let mut p = original();
        let c = chart_mut(&mut p);
        c.cat_hidden = true;
        c.cat_label_offset = offset;
        c.format.line_spacings.insert(Role::Default, Spacing::Pct(1.5));
        c.format.line_spacings.insert(Role::CategoryAxis, Spacing::Pts(40.));
        c.format.line_spacings.insert(Role::ValueAxis, Spacing::Pts(10.));
        let back = rebuild(&p);
        let c = chart(&back);
        assert!(c.cat_hidden);
        assert_eq!(c.cat_label_offset, offset);
        assert_eq!(c.format.line_spacings, chart(&p).format.line_spacings);
        assert_eq!(c.line_spacing(Role::Legend), Some(&Spacing::Pct(1.5)));
    }
    let c: Chart = serde_json::from_str(r#"{"kind":"column","format":{}}"#).unwrap();
    assert!(!c.cat_hidden);
    assert!(!c.cat_deleted);
    assert_eq!(c.cat_label_offset, None);
    assert_eq!(c.line_spacing(Role::CategoryAxis), None);
}

fn red_rows(p: &Presentation) -> Vec<u32> {
    let img = render_slide(p, 0, &RenderOpts::default());
    (0..img.height)
        .filter(|&y| {
            (0..img.width).any(|x| {
                let px = img.pixel(x, y);
                px[0] > 200 && px[1] < 40 && px[2] < 40
            })
        })
        .collect()
}

#[test]
fn hidden_category_labels_have_no_visible_pixels_after_rebuild() {
    let mut p = original();
    assert!(!red_rows(&rebuild(&p)).is_empty());
    chart_mut(&mut p).cat_hidden = true;
    assert!(red_rows(&rebuild(&p)).is_empty());
}

#[test]
fn hiding_category_text_preserves_the_axis_but_deleting_it_removes_both() {
    use deckcraft_model::chart::ChartLineRole;
    for (hidden, deleted) in [(false, false), (true, false), (false, true)] {
        let mut p = original();
        let c = chart_mut(&mut p);
        c.cat_hidden = hidden;
        c.cat_deleted = deleted;
        c.format.lines.insert(
            ChartLineRole::CategoryAxis,
            deckcraft_model::Line { fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(0, 0, 255)))), width: Some(3.), ..Default::default() },
        );
        let back = rebuild(&p);
        assert_eq!((chart(&back).cat_hidden, chart(&back).cat_deleted), (hidden, deleted));
        let img = render_slide(&back, 0, &RenderOpts::default());
        let blue = img.pixel(100, 320);
        assert_eq!(blue[0] < 40 && blue[1] < 40 && blue[2] > 200, !deleted, "axis line {hidden}/{deleted}: {blue:?}");
        assert_eq!(!red_rows(&back).is_empty(), !(hidden || deleted), "label visibility {hidden}/{deleted}");
    }
}

#[test]
fn explicit_category_line_spacing_changes_visible_line_distance() {
    let mut extents = Vec::new();
    for spacing in [20., 40.] {
        let mut p = original();
        chart_mut(&mut p).format.line_spacings.insert(Role::CategoryAxis, Spacing::Pts(spacing));
        // Both lines need room: the original taller plot truncates the second
        // 40pt line, so its total ink extent does not measure line spacing.
        chart_mut(&mut p).format.plot_layout.as_mut().unwrap().h = Some(0.5);
        let rows = red_rows(&rebuild(&p));
        extents.push(rows.last().unwrap() - rows.first().unwrap());
    }
    assert!((extents[1] as i32 - extents[0] as i32 - 20).abs() <= 1, "visible line distance: {extents:?}");
}

#[test]
fn category_tick_overflow_keeps_whole_lines_and_attaches_ellipsis_without_recentering() {
    use deckcraft_model::text::{BodyProps, TextOverflow};
    let mut bounds = Vec::new();
    for policy in [None, Some(TextOverflow::Clip), Some(TextOverflow::Overflow)] {
        let mut p = original();
        let c = chart_mut(&mut p);
        c.format.line_spacings.insert(Role::CategoryAxis, Spacing::Pts(40.));
        c.format.bodies.insert(Role::CategoryAxis, BodyProps { vert_overflow: policy, ..Default::default() });
        let back = rebuild(&p);
        assert_eq!(chart(&back).format.bodies.get(&Role::CategoryAxis).and_then(|body| body.vert_overflow), policy);
        assert_eq!(chart(&back).categories, ["A\nB"]);
        let img = render_slide(&back, 0, &RenderOpts::default());
        let mut bound = (u32::MAX, 0, 0);
        for y in 0..img.height {
            for x in 0..img.width {
                let px = img.pixel(x, y);
                if px[0] > 200 && px[1] < 40 && px[2] < 40 {
                    bound = (bound.0.min(x), bound.1.max(x), bound.2.max(y));
                }
            }
        }
        bounds.push(bound);
    }
    // The default A… and clipped A share the retained line's origin. Overflow
    // retains B, whose lower ink is still clipped at the chart's 400pt frame.
    assert_eq!(bounds[0].0, bounds[1].0);
    assert!(bounds[0].1 > bounds[1].1 + 10, "ellipsis: {bounds:?}");
    assert_eq!(bounds[0].2, bounds[1].2);
    assert!(bounds[0].2 < 370 && bounds[2].2 == 399, "line retention: {bounds:?}");
}

#[test]
fn category_tick_capacity_uses_complete_pitches_and_chart_label_margin() {
    for (font, spacing, count, heights) in [
        ("Inter", Some(40.), 2, [414.25, 414.75]),
        ("Lato", Some(32.), 2, [398.4, 398.9]),
        ("Inter", None, 2, [382.5, 383.]),
        ("Inter", Some(30.), 3, [424.25, 424.75]),
    ] {
        for (i, height) in heights.into_iter().enumerate() {
            let mut p = original();
            Arc::make_mut(&mut p.slides[0]).shapes[0].xfrm = Some(Xfrm::new(0., 0., 900., height));
            let c = chart_mut(&mut p);
            c.categories = vec![if count == 3 { "A\nB\nC" } else { "A\nB" }.into()];
            c.text_styles.get_mut(&Role::CategoryAxis).unwrap().font = Some(font.into());
            if let Some(spacing) = spacing {
                c.format.line_spacings.insert(Role::CategoryAxis, Spacing::Pts(spacing));
            }
            let plot = c.format.plot_layout.as_mut().unwrap();
            plot.y = Some(60. / height);
            plot.h = Some(260. / height);
            let rows = red_rows(&rebuild(&p));
            let lines = 1 + rows.windows(2).filter(|pair| pair[1] > pair[0] + 1).count();
            assert_eq!(lines, count - 1 + i, "{font} {spacing:?}, height {height}: {rows:?}");
        }
    }
}

#[test]
fn category_tick_capacity_rounds_exact_spacing_but_keeps_percentage_pitch() {
    for (font, spacing, height, expected) in [
        ("Inter", Spacing::Pts(30.4), 394.7, 2),
        ("Inter", Spacing::Pts(30.6), 396., 1),
        ("Inter", Spacing::Pct(1.3), 397.1, 1),
        ("Inter", Spacing::Pct(1.3), 397.6, 2),
        ("Lato", Spacing::Pts(32.4), 399., 2),
        ("Lato", Spacing::Pts(32.6), 400.2, 1),
    ] {
        let mut p = original();
        Arc::make_mut(&mut p.slides[0]).shapes[0].xfrm = Some(Xfrm::new(0., 0., 900., height));
        let c = chart_mut(&mut p);
        c.text_styles.get_mut(&Role::CategoryAxis).unwrap().font = Some(font.into());
        c.format.line_spacings.insert(Role::CategoryAxis, spacing);
        let plot = c.format.plot_layout.as_mut().unwrap();
        plot.y = Some(60. / height);
        plot.h = Some(260. / height);
        let rows = red_rows(&rebuild(&p));
        let lines = 1 + rows.windows(2).filter(|pair| pair[1] > pair[0] + 1).count();
        assert_eq!(lines, expected, "{font} {spacing:?}, height {height}: {rows:?}");
    }
}

#[test]
fn category_label_offset_moves_labels_by_a_quarter_natural_line_height() {
    let mut tops = Vec::new();
    for offset in [0, 100, 300] {
        let mut p = original();
        chart_mut(&mut p).categories = vec!["A".into()];
        chart_mut(&mut p).cat_label_offset = Some(offset);
        let back = rebuild(&p);
        assert_eq!(chart(&back).format.plot_layout, chart(&p).format.plot_layout);
        tops.push(*red_rows(&back).first().unwrap());
    }
    // Inter at 20 pt has a 24.2 pt natural line; the native independent controls
    // move six points for 100 and a further twelve points for 200.
    assert!((tops[1] as i32 - tops[0] as i32 - 6).abs() <= 1, "{tops:?}");
    assert!((tops[2] as i32 - tops[1] as i32 - 12).abs() <= 1, "{tops:?}");
}

#[test]
fn automatic_plot_reserves_authored_spacing_and_offset_without_moving_labels_outside() {
    let mut bottoms = Vec::new();
    let mut label_tops = Vec::new();
    for (offset, spacing) in [(0, None), (300, None), (100, Some(10.)), (100, Some(40.))] {
        let mut p = original();
        let c = chart_mut(&mut p);
        c.format.plot_layout = None;
        c.categories = vec!["A".into()];
        c.cat_label_offset = Some(offset);
        c.plot_fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(0, 255, 0))));
        if let Some(spacing) = spacing {
            c.format.line_spacings.insert(Role::CategoryAxis, Spacing::Pts(spacing));
        }
        let back = rebuild(&p);
        let img = render_slide(&back, 0, &RenderOpts::default());
        let bottom = img
            .pixels
            .chunks_exact(4)
            .enumerate()
            .filter(|(_, px)| px[0] < 20 && px[1] > 240 && px[2] < 20)
            .map(|(i, _)| i as u32 / img.width)
            .max()
            .unwrap();
        bottoms.push(bottom);
        label_tops.push(*red_rows(&back).first().unwrap());
    }
    assert!((bottoms[0] as i32 - bottoms[1] as i32 - 18).abs() <= 1, "offset gutter: {bottoms:?}");
    assert_eq!(label_tops[0], label_tops[1], "automatic label location remains fixed while its plot shrinks");
    assert!((bottoms[2] as i32 - bottoms[3] as i32 - 30).abs() <= 1, "exact line spacing gutter: {bottoms:?}");
}

#[test]
fn hidden_axes_leave_fixed_chart_margin_and_no_explicit_outer_reserve() {
    for outer in [false, true] {
        for size in [8., 36.] {
            let mut p = original();
            let c = chart_mut(&mut p);
            c.cat_hidden = true;
            c.y_hidden = true;
            c.text_styles.get_mut(&Role::Default).unwrap().size = Some(size);
            c.plot_fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(0, 255, 0))));
            if outer {
                c.format.plot_layout.as_mut().unwrap().inner = false;
            } else {
                c.format.plot_layout = None;
            }
            let back = rebuild(&p);
            let img = render_slide(&back, 0, &RenderOpts::default());
            let green: Vec<_> = img
                .pixels
                .chunks_exact(4)
                .enumerate()
                .filter(|(_, px)| px[0] < 20 && px[1] > 240 && px[2] < 20)
                .map(|(i, _)| (i as u32 % img.width, i as u32 / img.width))
                .collect();
            let actual = [
                green.iter().map(|p| p.0).min().unwrap(),
                green.iter().map(|p| p.1).min().unwrap(),
                green.iter().map(|p| p.0).max().unwrap() + 1,
                green.iter().map(|p| p.1).max().unwrap() + 1,
            ];
            let expected = if outer { [90, 60, 720, 320] } else { [11, 11, 889, 389] };
            assert!(actual.iter().zip(expected).all(|(a, b)| (*a as i32 - b).abs() <= 1), "{actual:?} vs {expected:?}");
        }
    }
}
