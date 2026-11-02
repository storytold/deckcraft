//! Original chart-label controls: stored text survives while automatic labels clip or
//! ellipsize whole lines. Painted colours verify the marker separately from text import.
use std::sync::Arc;

use deckcraft_color::Rgba;
use deckcraft_engine::render::{RenderOpts, render_slide};
use deckcraft_model::chart::{Chart, ChartLayout, ChartTextRole, ChartType, DataLabel, Series};
use deckcraft_model::text::{Align, Anchor, Run, RunProps, TextBody, TextOverflow};
use deckcraft_model::{ColorRef, Fill, Presentation, Shape, ShapeId, ShapeKind, Slide, Xfrm};

fn original(policy: Option<TextOverflow>) -> Presentation {
    let mut chart = Chart::sample(ChartType::Column);
    chart.categories = vec!["A".into()];
    chart.title = None;
    chart.legend = None;
    chart.y_max = Some(10.);
    chart.text_styles.insert(ChartTextRole::Default, RunProps { font: Some("Inter".into()), size: Some(16.), ..Default::default() });
    let mut body = TextBody::from_text(&(1..=12).map(|i| format!("Row{i:02}")).collect::<Vec<_>>().join("\n"));
    body.body.vertical_overflow = policy;
    for (i, p) in body.paragraphs.iter_mut().enumerate() {
        p.runs[0].props = RunProps {
            font: Some("Inter".into()),
            size: Some(30.),
            fill: Some(Fill::solid(ColorRef::rgb(if i < 5 { Rgba::rgb(255, 0, 0) } else { Rgba::rgb(0, 0, 255) }))),
            ..Default::default()
        };
    }
    let mut series = Series { values: vec![Some(7.)], fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(180, 180, 180)))), ..Default::default() };
    series.point_labels.insert(0, DataLabel { text: Some(body), position: Some("ctr".into()), ..Default::default() });
    chart.series = vec![series];
    Presentation {
        slides: vec![Arc::new(Slide {
            shapes: vec![Shape {
                id: ShapeId(1),
                xfrm: Some(Xfrm::new(0., 0., 960., 400.)),
                kind: ShapeKind::Chart(Box::new(chart)),
                ..Default::default()
            }],
            ..Default::default()
        })],
        ..Default::default()
    }
}

#[test]
fn overflow_policy_and_complete_rich_body_survive_forced_rebuild() {
    for policy in [None, Some(TextOverflow::Overflow), Some(TextOverflow::Clip), Some(TextOverflow::Ellipsis)] {
        let p = original(policy);
        let mut back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
        let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut back.slides[0]).shapes[0].kind else { panic!("chart") };
        chart.raw = None;
        chart.kept = Default::default();
        let bytes = deckcraft_pptx::export(&back).unwrap();
        let back = deckcraft_pptx::import(&bytes).unwrap();
        let ShapeKind::Chart(chart) = &back.slides[0].shapes[0].kind else { panic!("chart") };
        let text = chart.series[0].point_labels[&0].text.as_ref().unwrap();
        assert_eq!(text.body.vertical_overflow, policy);
        assert_eq!(text.paragraphs.len(), 12);
        assert!(text.text().ends_with("Row12"));

        let img = render_slide(&back, 0, &RenderOpts::default());
        let blue: Vec<_> = img.pixels.chunks_exact(4).enumerate().filter(|(_, px)| px[2] > 200 && px[0] < 40 && px[1] < 40).collect();
        if policy == Some(TextOverflow::Clip) {
            assert!(blue.is_empty(), "clip must omit all hidden runs and the marker");
        } else {
            assert!(!blue.is_empty(), "ellipsis takes the hidden run's blue fill");
            let top = blue.iter().map(|(i, _)| *i as u32 / img.width).min().unwrap();
            let bottom = blue.iter().map(|(i, _)| *i as u32 / img.width).max().unwrap();
            if policy == Some(TextOverflow::Overflow) {
                assert!(bottom - top > 40, "explicit overflow retains subsequent blue lines");
                assert!(bottom < 400, "overflow is clipped at the chart frame");
            } else {
                assert!(bottom - top < 30, "only the blue ellipsis is visible");
            }
        }
        if let Some(dir) = std::env::var_os("DECKCRAFT_RENDER_PROBES") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("label-overflow-{}.pptx", policy.map_or("default", TextOverflow::xml))), bytes).unwrap();
        }
    }
}

fn manual(w: Option<f64>, h: Option<f64>, angle: f64, anchor: Anchor) -> Presentation {
    let mut p = original(None);
    let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
    let label = chart.series[0].point_labels.get_mut(&0).unwrap();
    label.layout = Some(ChartLayout { w, h, ..Default::default() });
    label.fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(255, 255, 0))));
    let body = label.text.as_mut().unwrap();
    body.body.rot = Some(angle);
    body.body.anchor = Some(anchor);
    p
}

fn painted_bounds(p: &Presentation, yellow: bool) -> Option<(u32, u32, u32, u32)> {
    let img = render_slide(p, 0, &RenderOpts::default());
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for (i, px) in img.pixels.chunks_exact(4).enumerate() {
        if px[0] > 200 && px[2] < 40 && (if yellow { px[1] > 200 } else { px[1] < 40 }) {
            let (x, y) = (i as u32 % img.width, i as u32 / img.width);
            bounds = Some(bounds.map_or((x, y, x, y), |b| (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y))));
        }
    }
    bounds
}

#[test]
fn manual_dimensions_and_rotation_survive_rebuild_and_rotate_the_background() {
    for angle in [0., 30., 90.] {
        let p = manual(Some(0.2), Some(0.25), angle, Anchor::Middle);
        let mut back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
        let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut back.slides[0]).shapes[0].kind else { panic!("chart") };
        chart.raw = None;
        chart.kept = Default::default();
        let back = deckcraft_pptx::import(&deckcraft_pptx::export(&back).unwrap()).unwrap();
        let ShapeKind::Chart(chart) = &back.slides[0].shapes[0].kind else { panic!("chart") };
        let label = &chart.series[0].point_labels[&0];
        assert_eq!(label.layout.as_ref().unwrap().w, Some(0.2));
        assert_eq!(label.layout.as_ref().unwrap().h, Some(0.25));
        assert_eq!(label.text.as_ref().unwrap().body.rot, Some(angle));
        assert_eq!(label.text.as_ref().unwrap().paragraphs.len(), 12);
        let b = painted_bounds(&back, true).unwrap();
        let (s, c) = angle.to_radians().sin_cos();
        let expected = (192. * c + 100. * s, 192. * s + 100. * c);
        assert!(((b.2 - b.0 + 1) as f64 - expected.0).abs() < 2.);
        assert!(((b.3 - b.1 + 1) as f64 - expected.1).abs() < 2.);
    }
}

#[test]
fn missing_manual_extent_is_empty_but_positive_narrow_boxes_keep_ink() {
    for (w, h) in [(None, Some(0.25)), (Some(0.2), None), (Some(0.), Some(0.25)), (Some(0.2), Some(0.))] {
        let p = manual(w, h, 30., Anchor::Middle);
        assert!(painted_bounds(&p, true).is_none());
        assert!(painted_bounds(&p, false).is_none());
    }
    let p = manual(Some(0.001), Some(0.25), 0., Anchor::Middle);
    let ink = painted_bounds(&p, false).unwrap();
    assert!(ink.2 - ink.0 > 3, "horizontal overflow of the first glyph is retained");
}

#[test]
fn manual_height_preserves_explicit_anchor_and_clips_oversized_ink() {
    let mut positions = Vec::new();
    for anchor in [Anchor::Top, Anchor::Middle, Anchor::Bottom] {
        let mut p = manual(Some(0.2), Some(0.5), 0., anchor);
        let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
        chart.series[0].point_labels.get_mut(&0).unwrap().text.as_mut().unwrap().paragraphs.truncate(1);
        positions.push(painted_bounds(&p, false).unwrap().1);
    }
    assert!(positions[1] > positions[0] + 60 && positions[2] > positions[1] + 60);
    let p = manual(Some(0.2), Some(0.01), 0., Anchor::Middle);
    let ink = painted_bounds(&p, false).unwrap();
    assert!(ink.3 - ink.1 <= 4, "an oversized first line is vertically clipped to the manual box");
}

#[test]
fn manual_overflow_centers_the_complete_body_across_the_fixed_box() {
    let mut tops = Vec::new();
    for anchor in [Anchor::Top, Anchor::Middle] {
        let mut p = manual(Some(0.2), Some(0.25), 0., anchor);
        let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
        let body = chart.series[0].point_labels.get_mut(&0).unwrap().text.as_mut().unwrap();
        body.body.vertical_overflow = Some(TextOverflow::Overflow);
        let background = painted_bounds(&p, true).unwrap();
        let ink = painted_bounds(&p, false).unwrap();
        tops.push(ink.1);
        if anchor == Anchor::Top {
            assert!(ink.1 >= background.1);
        } else {
            assert!(ink.1 + 100 < background.1, "centered overflow extends above the fixed background");
        }
    }
    assert!(tops[1] + 100 < tops[0]);
}

fn ellipsis_control(align: Align, rtl: bool, marker_size: f64) -> Presentation {
    let mut p = original(None);
    let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
    shape.xfrm = Some(Xfrm::new(0., 0., 1000., 260.));
    let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
    chart.y_min = Some(0.);
    chart.format.plot_layout =
        Some(ChartLayout { inner: true, x: Some(0.1), y: Some(0.15), w: Some(0.7), h: Some(0.65), x_edge: true, y_edge: true, ..Default::default() });
    let label = chart.series[0].point_labels.get_mut(&0).unwrap();
    label.fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(255, 255, 0))));
    let mut body = TextBody::from_text("I\nI\nWWWWWW\nZ");
    body.body.inset_l = Some(3.);
    body.body.inset_r = Some(3.);
    body.body.inset_t = Some(1.5);
    body.body.inset_b = Some(1.5);
    body.body.wrap = Some(true);
    for (i, para) in body.paragraphs.iter_mut().enumerate() {
        para.props.align = Some(align);
        para.props.rtl = Some(rtl);
        para.runs[0].props = RunProps {
            font: Some("Inter".into()),
            size: Some(if i == 3 { marker_size } else { 30. }),
            fill: Some(Fill::solid(ColorRef::rgb(if i == 3 { Rgba::rgb(0, 0, 255) } else { Rgba::rgb(255, 0, 0) }))),
            ..Default::default()
        };
    }
    label.text = Some(body);
    p
}

fn rebuilt(p: &Presentation) -> Presentation {
    let mut back = deckcraft_pptx::import(&deckcraft_pptx::export(p).unwrap()).unwrap();
    let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut back.slides[0]).shapes[0].kind else { panic!("chart") };
    chart.raw = None;
    chart.kept = Default::default();
    deckcraft_pptx::import(&deckcraft_pptx::export(&back).unwrap()).unwrap()
}

#[test]
fn automatic_marker_reservation_and_background_bounds_are_distinct_in_ltr_and_rtl() {
    for rtl in [false, true] {
        for align in [Align::Left, Align::Center, Align::Right] {
            for marker_size in [30., 60.] {
                let p = rebuilt(&ellipsis_control(align, rtl, marker_size));
                let b = painted_bounds(&p, true).unwrap();
                // Independent native controls with a fixed inner plot. The automatic
                // marker participates in alignment, but is excluded from background bounds.
                let full = (!rtl && align == Align::Right) || (rtl && align == Align::Left);
                let expected = match (full, marker_size == 60.) {
                    (false, false) => 153.8,
                    (false, true) => 124.24,
                    (true, false) => 179.73,
                    (true, true) => 176.095,
                };
                assert!(((b.2 - b.0 + 1) as f64 - expected).abs() <= 2., "{align:?} rtl={rtl} marker={marker_size}: {b:?}");
                assert!(((b.0 + b.2 + 1) as f64 / 2. - 450.).abs() <= 0.5);
                let img = render_slide(&p, 0, &RenderOpts::default());
                assert!(!img.pixels.chunks_exact(4).any(|px| px[2] > 200 && px[0] < 40 && px[1] < 40), "paint uses the removed W's red style");
                let ShapeKind::Chart(chart) = &p.slides[0].shapes[0].kind else { panic!("chart") };
                assert_eq!(chart.series[0].point_labels[&0].text.as_ref().unwrap().text(), "I\nI\nWWWWWW\nZ");
            }
        }
    }
}

#[test]
fn a_marker_takes_the_first_removed_runs_style_without_refitting_its_reserved_width() {
    let mut p = ellipsis_control(Align::Center, false, 60.);
    let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
    let body = chart.series[0].point_labels.get_mut(&0).unwrap().text.as_mut().unwrap();
    body.paragraphs[2].runs[0].text = "AAAAAA".into();
    let props = RunProps { fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(0, 255, 0)))), ..body.paragraphs[2].runs[0].props.clone() };
    body.paragraphs[2].runs.push(Run::with("B", props));
    let p = rebuilt(&p);
    let img = render_slide(&p, 0, &RenderOpts::default());
    let green_rows: Vec<_> =
        img.pixels.chunks_exact(4).enumerate().filter(|(_, px)| px[1] > 200 && px[0] < 40 && px[2] < 40).map(|(i, _)| i as u32 / img.width).collect();
    let height = green_rows.iter().max().unwrap() - green_rows.iter().min().unwrap() + 1;
    assert!(height <= 5, "only the green ellipsis remains, not the removed B: height {height}");
    assert!(!img.pixels.chunks_exact(4).any(|px| px[2] > 200 && px[0] < 40 && px[1] < 40));
    let b = painted_bounds(&p, true).unwrap();
    assert!(((b.2 - b.0 + 1) as f64 - 130.2).abs() <= 1.5);
}

#[test]
fn a_negative_quarter_turn_centers_the_unmarked_background_on_the_fixed_plot_bar() {
    for (w, h, expected) in
        [(960., 400., (340., 144., 524., 262.)), (720., 240., (268., 67.5, 379.5, 176.)), (500., 720., (187.5, 308., 262.5, 422.5))]
    {
        for angle in [-90., 270.] {
            let mut p = original(None);
            let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
            shape.xfrm = Some(Xfrm::new(0., 0., w, h));
            let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
            chart.y_min = Some(0.);
            chart.series[0].values = vec![Some(9.)];
            chart.format.plot_layout = Some(ChartLayout {
                inner: true,
                x: Some(0.1),
                y: Some(0.15),
                w: Some(0.7),
                h: Some(0.65),
                x_edge: true,
                y_edge: true,
                ..Default::default()
            });
            let label = chart.series[0].point_labels.get_mut(&0).unwrap();
            label.fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(255, 255, 0))));
            let body = label.text.as_mut().unwrap();
            body.body.rot = Some(angle);
            body.body.wrap = Some(true);
            let b = painted_bounds(&rebuilt(&p), true).unwrap();
            // Native controls keep the automatic marker outside this background.
            for (actual, expected) in [(b.0, expected.0), (b.1, expected.1), (b.2 + 1, expected.2), (b.3 + 1, expected.3)] {
                assert!((actual as f64 - expected).abs() <= 1.5, "{w}x{h} at {angle}: {b:?}");
            }
        }
    }
}

#[test]
fn automatic_half_turns_keep_the_full_unwrapped_body_for_every_overflow_policy() {
    for wrap in [false, true] {
        let mut images = Vec::new();
        for angle in [-180., 180.] {
            for policy in [None, Some(TextOverflow::Clip), Some(TextOverflow::Ellipsis), Some(TextOverflow::Overflow)] {
                let mut p = original(policy);
                let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
                shape.xfrm = Some(Xfrm::new(0., 0., 400., 360.));
                let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
                chart.y_min = Some(0.);
                chart.series[0].values = vec![Some(9.)];
                chart.format.plot_layout = Some(ChartLayout {
                    inner: true,
                    x: Some(0.1),
                    y: Some(0.15),
                    w: Some(0.7),
                    h: Some(0.65),
                    x_edge: true,
                    y_edge: true,
                    ..Default::default()
                });
                let label = chart.series[0].point_labels.get_mut(&0).unwrap();
                label.fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(255, 255, 0))));
                let body = label.text.as_mut().unwrap();
                body.body.rot = Some(angle);
                body.body.wrap = Some(wrap);
                let back = rebuilt(&p);
                let b = painted_bounds(&back, true).unwrap();
                // Native controls ignore the overflow policy at a half turn and
                // retain full row widths beyond the 80pt automatic wrap budget.
                // Explicit no-wrap centers the full box; wrapping uses its cap.
                let expected = if wrap { (140., 92.7, 246.15, 360.) } else { (126.925, 0., 233.075, 360.) };
                for (actual, expected) in [(b.0, expected.0), (b.1, expected.1), (b.2 + 1, expected.2), (b.3 + 1, expected.3)] {
                    assert!((actual as f64 - expected).abs() <= 1.5, "{angle}, wrap={wrap}, policy={policy:?}: {b:?}");
                }
                let ShapeKind::Chart(chart) = &back.slides[0].shapes[0].kind else { panic!("chart") };
                let body = chart.series[0].point_labels[&0].text.as_ref().unwrap();
                assert_eq!(body.body.vertical_overflow, policy);
                assert_eq!(body.paragraphs.len(), 12);
                images.push(render_slide(&back, 0, &RenderOpts::default()).pixels);
                if angle == 180. && !wrap && policy == Some(TextOverflow::Overflow) {
                    let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
                    chart.series[0].point_labels.get_mut(&0).unwrap().text.as_mut().unwrap().body.rot = Some(0.);
                    assert_eq!(painted_bounds(&rebuilt(&p), true), Some(b), "unrotated no-wrap overflow centers the same full box");
                }
            }
        }
        assert!(images.windows(2).all(|pair| pair[0] == pair[1]), "half-turn policy/sign invariance, wrap={wrap}");
    }
}
