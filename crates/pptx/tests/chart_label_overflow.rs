//! Original chart-label controls: stored text survives while automatic labels clip or
//! ellipsize whole lines. Painted colours verify the marker separately from text import.
use std::sync::Arc;

use deckcraft_color::Rgba;
use deckcraft_engine::render::{RenderOpts, render_slide};
use deckcraft_model::chart::{Chart, ChartAxisCrossing, ChartLayout, ChartLineRole, ChartTextRole, ChartTickPosition, ChartType, DataLabel, Series};
use deckcraft_model::text::{Align, Anchor, Run, RunProps, TextBody, TextOverflow};
use deckcraft_model::{ColorRef, Fill, Line, Presentation, Shape, ShapeId, ShapeKind, Slide, Xfrm};

fn original(policy: Option<TextOverflow>) -> Presentation {
    let mut chart = Chart::sample(ChartType::Column);
    chart.categories = vec!["A".into()];
    chart.title = None;
    chart.legend = None;
    chart.y_max = Some(10.);
    chart.text_styles.insert(ChartTextRole::Default, RunProps { font: Some("Inter".into()), size: Some(16.), ..Default::default() });
    let mut body = TextBody::from_text(&(1..=12).map(|i| format!("Row{i:02}")).collect::<Vec<_>>().join("\n"));
    body.body.vert_overflow = policy;
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
        assert_eq!(text.body.vert_overflow, policy);
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
        body.body.vert_overflow = Some(TextOverflow::Overflow);
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
                assert_eq!(body.body.vert_overflow, policy);
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

#[test]
fn rich_column_advances_control_wrapping_and_painted_width_after_rebuild() {
    for (font, size, text, width, wrap, expected_width) in [
        ("Inter", 30., "Row10".into(), 400., true, 67.845),
        ("Inter", 30., "Row10".into(), 400.225, true, 80.045),
        ("Inter", 30., "Row1a".into(), 400., true, 67.845),
        ("Lato", 22., "IIIIIIIIX".into(), 275.7, true, 49.12),
        ("Lato", 22., "IIIIIIIIX".into(), 276.5, true, 55.28),
        ("Inter", 30., "I".repeat(48), 960., false, 392.64),
        ("Lato", 22., "I".repeat(48), 960., false, 301.68),
    ] {
        let mut p = original(Some(TextOverflow::Overflow));
        let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
        shape.xfrm = Some(Xfrm::new(0., 0., width, 360.));
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
        body.body.wrap = Some(wrap);
        body.paragraphs.truncate(1);
        body.paragraphs[0].runs[0].text = text.clone();
        body.paragraphs[0].runs[0].props.font = Some(font.into());
        body.paragraphs[0].runs[0].props.size = Some(size);
        let back = rebuilt(&p);
        let image = render_slide(&back, 0, &RenderOpts { scale: 4., ..Default::default() });
        let columns: Vec<_> = image
            .pixels
            .chunks_exact(4)
            .enumerate()
            .filter(|(_, px)| px[0] > 200 && px[1] > 200 && px[2] < 40)
            .map(|(i, _)| i as u32 % image.width)
            .collect();
        let painted_width = (columns.iter().max().unwrap() - columns.iter().min().unwrap() + 1) as f64 / 4.;
        assert!((painted_width - expected_width).abs() <= 0.5, "{font} {text}, width={width}: painted width {painted_width}");
        assert!(painted_bounds(&back, false).is_some());
        let ShapeKind::Chart(chart) = &back.slides[0].shapes[0].kind else { panic!("chart") };
        assert_eq!(chart.series[0].point_labels[&0].text.as_ref().unwrap().text(), text);
    }
}

#[test]
fn manual_labels_center_the_retained_block_and_keep_authored_anchor_ctr() {
    for (first, rest, anchor_ctr, inset, expected_x) in [
        (Align::Center, Align::Left, Some(false), 3., 324.53),
        (Align::Center, Align::Right, Some(false), 3., 324.53),
        (Align::Left, Align::Center, Some(false), 3., 303.),
        (Align::Left, Align::Center, None, 3., 324.53),
        (Align::Right, Align::Center, Some(false), 3., 346.06),
        (Align::Right, Align::Center, Some(true), 3., 324.53),
        (Align::Center, Align::Left, None, 13., 329.53),
    ] {
        let mut p = original(None);
        let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
        shape.xfrm = Some(Xfrm::new(0., 0., 800., 360.));
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
        label.layout = Some(ChartLayout { w: Some(0.15), h: Some(0.18), ..Default::default() });
        label.fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(255, 255, 0))));
        let body = label.text.as_mut().unwrap();
        body.body.anchor_ctr = anchor_ctr;
        body.body.inset_l = Some(inset);
        body.paragraphs.truncate(4);
        for (i, para) in body.paragraphs.iter_mut().enumerate() {
            para.props.align = Some(if i == 0 { first } else { rest });
            para.runs[0].text = if i == 0 { "WWWW" } else { "I" }.into();
            para.runs[0].props.size = Some(18.);
            para.runs[0].props.fill = Some(Fill::solid(ColorRef::rgb(if i == 0 { Rgba::rgb(255, 0, 0) } else { Rgba::rgb(0, 0, 255) })));
        }
        let back = rebuilt(&p);
        let red = painted_bounds(&back, false).unwrap();
        // First-line ink, independently of the blue last line and attached marker.
        assert!((red.0 as f64 - expected_x).abs() <= 1.5, "{first:?}/{rest:?}, anchorCtr={anchor_ctr:?}: {red:?}");
        let bg = painted_bounds(&back, true).unwrap();
        assert!((bg.0 as f64 - 300.).abs() <= 1. && (bg.2 as f64 + 1. - 420.).abs() <= 1.);
        let ShapeKind::Chart(chart) = &back.slides[0].shapes[0].kind else { panic!("chart") };
        let body = chart.series[0].point_labels[&0].text.as_ref().unwrap();
        assert_eq!(body.body.anchor_ctr, anchor_ctr);
        assert_eq!(body.paragraphs[0].props.align, Some(first));
        // The chart reader already resolves the literal first-paragraph alignment.
        assert!(body.paragraphs[1..].iter().all(|p| p.props.align == Some(first)));
        assert_eq!(body.text(), "WWWW\nI\nI\nI");
    }
}

fn baseline_control(value: f64, size: f64) -> Presentation {
    let mut p = original(None);
    let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
    chart.y_min = Some(if value < 0. { -10. } else { 0. });
    chart.y_max = Some(if value < 0. { 0. } else { 10. });
    chart.series[0].values = vec![Some(value)];
    chart.format.plot_layout =
        Some(ChartLayout { inner: true, x: Some(0.1), y: Some(0.15), w: Some(0.7), h: Some(0.65), x_edge: true, y_edge: true, ..Default::default() });
    let label = chart.series[0].point_labels.get_mut(&0).unwrap();
    label.fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(255, 255, 0))));
    for para in &mut label.text.as_mut().unwrap().paragraphs {
        para.runs[0].props.size = Some(size);
    }
    p
}

#[test]
fn centered_labels_keep_their_background_inside_the_positive_or_negative_baseline() {
    for (value, top, bottom) in [
        (1., 132.525, 317.),
        (3., 132.525, 317.),
        (7., 132.525, 317.),
        (9., 110.7625, 295.2375),
        (-1., 63., 247.475),
        (-3., 63., 247.475),
        (-7., 63., 247.475),
        (-9., 84.7625, 269.2375),
    ] {
        let back = rebuilt(&baseline_control(value, 30.));
        let bg = painted_bounds(&back, true).unwrap();
        assert!((bg.1 as f64 - top).abs() <= 1.5 && (bg.3 as f64 + 1. - bottom).abs() <= 1.5, "value={value}: {bg:?}");
        assert!(painted_bounds(&back, false).is_some());
    }
}

#[test]
fn baseline_placement_preserves_wrapped_and_unwrapped_overflow_policies() {
    for value in [1., -1.] {
        for (angle, wrap) in [(0., true), (37., true), (180., true), (0., false)] {
            let mut p = baseline_control(value, 18.);
            let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
            let body = chart.series[0].point_labels.get_mut(&0).unwrap().text.as_mut().unwrap();
            body.body.wrap = Some(wrap);
            body.body.rot = Some(angle);
            body.body.vert_overflow = Some(TextOverflow::Overflow);
            let back = rebuilt(&p);
            let bg = painted_bounds(&back, true).unwrap();
            let expected_top = if wrap {
                if value > 0. { 117. } else { 63. }
            } else if value > 0. {
                84.85
            } else {
                30.85
            };
            assert!((bg.1 as f64 - expected_top).abs() <= 1.5, "value={value}, angle={angle}, wrap={wrap}: {bg:?}");
            assert!(bg.3 - bg.1 > 240, "overflow retains the complete body after positioning its cap");
            let ShapeKind::Chart(chart) = &back.slides[0].shapes[0].kind else { panic!("chart") };
            let body = chart.series[0].point_labels[&0].text.as_ref().unwrap();
            assert_eq!(body.body.wrap, Some(wrap));
            assert_eq!(body.body.vert_overflow, Some(TextOverflow::Overflow));
            assert_eq!(body.paragraphs.len(), 12);
        }
    }
}

#[test]
fn an_oversized_first_line_extends_from_the_positioned_cap_for_every_overflow_policy() {
    for (font, size, angle, policy, value) in [
        ("Inter", 48., 0., None, 7.),
        ("Inter", 48., 0., Some(TextOverflow::Clip), 7.),
        ("Lato", 44., 0., None, 7.),
        ("Inter", 48., 37., None, 7.),
        ("Inter", 48., 37., None, -7.),
        ("Inter", 48., 0., Some(TextOverflow::Overflow), 7.),
    ] {
        let mut p = baseline_control(value, size);
        let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
        shape.xfrm = Some(Xfrm::new(0., 0., 800., 100.));
        let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
        let body = chart.series[0].point_labels.get_mut(&0).unwrap().text.as_mut().unwrap();
        body.body.rot = Some(angle);
        body.body.vert_overflow = policy;
        body.paragraphs.truncate(2);
        for (i, p) in body.paragraphs.iter_mut().enumerate() {
            p.runs[0].text = "I".into();
            p.runs[0].props.font = Some(font.into());
            p.runs[0].props.size = Some(if i == 0 { size } else { 12. });
        }
        let bg = painted_bounds(&rebuilt(&p), true).unwrap();
        let expected_top = if value > 0. { 27. } else { 18. };
        assert!((bg.1 as f64 - expected_top).abs() <= 1.5, "{font}, {angle}, {policy:?}, value={value}: {bg:?}");
        assert!(bg.3 - bg.1 > 50, "the retained first line is taller than the cap");
    }
}

#[test]
fn rich_column_vertical_margins_affect_paint_without_changing_authored_properties() {
    for (fixed, anchor, angle, font, size, top, short) in [
        (false, Anchor::Middle, 0., "Inter", 18., Some(7.), false),
        (false, Anchor::Middle, 0., "Inter", 18., None, false),
        (false, Anchor::Middle, 37., "Lato", 22., Some(7.), false),
        (true, Anchor::Middle, 0., "Lato", 22., Some(7.), false),
        (true, Anchor::Top, 0., "Open Sans", 21., Some(6.), true),
        (true, Anchor::Middle, 0., "Open Sans", 21., Some(6.), true),
        (true, Anchor::Bottom, 0., "Open Sans", 21., Some(6.), true),
        (false, Anchor::Bottom, 37., "Open Sans", 21., Some(6.), true),
    ] {
        let mut images = Vec::new();
        for (bottom, right) in [(1., 1.), (25., 1.), (25., 9.)] {
            let mut p = baseline_control(7., size);
            let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
            shape.xfrm = Some(Xfrm::new(0., 0., 700., 300.));
            let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
            let label = chart.series[0].point_labels.get_mut(&0).unwrap();
            if fixed {
                label.layout = Some(ChartLayout { w: Some(0.28), h: Some(0.5), ..Default::default() });
            }
            let body = label.text.as_mut().unwrap();
            body.body.anchor = Some(anchor);
            body.body.rot = Some(angle);
            body.body.inset_t = top;
            body.body.inset_b = Some(bottom);
            body.body.inset_l = Some(5.);
            body.body.inset_r = Some(right);
            if short {
                body.paragraphs.truncate(2);
            }
            for (i, para) in body.paragraphs.iter_mut().enumerate() {
                para.runs[0].props.font = Some(font.into());
                if short {
                    para.runs[0].text = if i == 0 { "Orbit" } else { "Pulse" }.into();
                }
            }
            let back = rebuilt(&p);
            assert!(painted_bounds(&back, false).is_some());
            images.push(render_slide(&back, 0, &RenderOpts::default()).pixels);
            let ShapeKind::Chart(chart) = &back.slides[0].shapes[0].kind else { panic!("chart") };
            let body = chart.series[0].point_labels[&0].text.as_ref().unwrap();
            assert_eq!(body.body.inset_t, top);
            assert_eq!(body.body.inset_b, Some(bottom));
            assert_eq!(body.body.inset_r, Some(right));
            assert_eq!(body.body.anchor, Some(anchor));
        }
        assert!(images[0] == images[1], "bottom margin is ignored: {font}, fixed={fixed}, {anchor:?}, angle={angle}");
        assert!(images[1] != images[2], "right margin remains independent: {font}, fixed={fixed}, {anchor:?}, angle={angle}");
    }
}

fn custom_margin_control(value: f64, font: &str, size: f64, angle: f64, bottom: Option<f64>) -> Presentation {
    let mut p = baseline_control(value, size);
    let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
    shape.xfrm = Some(Xfrm::new(0., 0., 840., 360.));
    let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
    chart.format.plot_layout = Some(ChartLayout {
        inner: true,
        x: Some(0.12),
        y: Some(0.11),
        w: Some(0.72),
        h: Some(0.74),
        x_edge: true,
        y_edge: true,
        ..Default::default()
    });
    let body = chart.series[0].point_labels.get_mut(&0).unwrap().text.as_mut().unwrap();
    body.body.rot = Some(angle);
    body.body.wrap = Some(true);
    body.body.inset_b = bottom;
    for para in &mut body.paragraphs {
        para.runs[0].props.font = Some(font.into());
    }
    p
}

#[test]
fn a_custom_bottom_margin_selects_full_box_overflow_placement_before_normalization() {
    let custom = 1.5 + 1. / 12700.;
    // Painted extents include the chart clip: the default-margin box's full
    // rotated left corner extends below the chart and is therefore not visible.
    for (value, bottom, expected_left, expected_top) in [(7., None, 350., 123.), (7., Some(custom), 271., 54.), (1., Some(custom), 271., 54.)] {
        let mut p = custom_margin_control(value, "Open Sans", 21., 37., bottom);
        let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
        chart.series[0].point_labels.get_mut(&0).unwrap().text.as_mut().unwrap().body.vert_overflow = Some(TextOverflow::Overflow);
        let back = rebuilt(&p);
        let bg = painted_bounds(&back, true).unwrap();
        assert!((bg.0 as f64 - expected_left).abs() < 1.5 && (bg.1 as f64 - expected_top).abs() < 1.5, "value={value}, bottom={bottom:?}: {bg:?}");
        assert!(painted_bounds(&back, false).is_some());
        let ShapeKind::Chart(chart) = &back.slides[0].shapes[0].kind else { panic!("chart") };
        let body = chart.series[0].point_labels[&0].text.as_ref().unwrap();
        assert_eq!(body.body.inset_t, None);
        assert_eq!(body.body.inset_b.is_some(), bottom.is_some());
        assert!((body.body.inset_b.unwrap_or(0.) - bottom.unwrap_or(0.)).abs() < 1e-9);
        assert_eq!(body.body.vert_overflow, Some(TextOverflow::Overflow));
        assert_eq!(body.paragraphs.len(), 12);
    }
}

#[test]
fn a_custom_half_turn_measures_narrowly_then_paints_at_the_grown_width() {
    let bottom = 1.5 + 1. / 12700.;
    let mut p = custom_margin_control(7., "Inter", 24., 180., Some(bottom));
    let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
    let body = chart.series[0].point_labels.get_mut(&0).unwrap().text.as_mut().unwrap();
    body.paragraphs.truncate(1);
    body.paragraphs[0].runs[0].text = "IIIIW".into();
    let back = rebuilt(&p);
    let bg = painted_bounds(&back, true).unwrap();
    for (actual, expected) in [(bg.0, 388.375), (bg.1, 138.6725), (bg.2 + 1, 418.025), (bg.3 + 1, 286.8475)] {
        assert!((actual as f64 - expected).abs() < 1.5, "five measured lines determine the box: {bg:?}");
    }
    let image = render_slide(&back, 0, &RenderOpts::default());
    let rows: Vec<_> = image
        .pixels
        .chunks_exact(image.width as usize * 4)
        .map(|row| row.chunks_exact(4).any(|px| px[0] > 200 && px[1] < 40 && px[2] < 40))
        .collect();
    let bands = rows.iter().enumerate().filter(|&(i, &ink)| ink && (i == 0 || !rows[i - 1])).count();
    assert_eq!(bands, 3, "the grown width paints III, I, W on three baselines");
    let ShapeKind::Chart(chart) = &back.slides[0].shapes[0].kind else { panic!("chart") };
    let body = chart.series[0].point_labels[&0].text.as_ref().unwrap();
    assert_eq!(body.text(), "IIIIW");
    assert_eq!(body.body.wrap, Some(true));
    assert_eq!(body.body.vert_overflow, None);
    assert!((body.body.inset_b.unwrap() - bottom).abs() < 1e-9);
}

fn layer_control(kind: ChartType, angle: f64) -> Presentation {
    let mut p = custom_margin_control(7., "Open Sans", 21., angle, Some(1.5 + 1. / 12700.));
    let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
    chart.kind = kind;
    chart.y_major = Some(1.);
    for role in [ChartTextRole::CategoryAxis, ChartTextRole::ValueAxis] {
        chart.text_styles.insert(
            role,
            RunProps { font: Some("Inter".into()), size: Some(16.), fill: Some(Fill::solid(ColorRef::rgb(Rgba::BLACK))), ..Default::default() },
        );
    }
    for (role, color) in [
        (ChartLineRole::CategoryAxis, Rgba::rgb(0, 170, 170)),
        (ChartLineRole::ValueAxis, Rgba::rgb(0, 170, 170)),
        (ChartLineRole::MajorGrid, Rgba::rgb(192, 0, 192)),
    ] {
        chart.format.lines.insert(role, Line { fill: Some(Fill::solid(ColorRef::rgb(color))), width: Some(4.), ..Default::default() });
    }
    let label = chart.series[0].point_labels.get_mut(&0).unwrap();
    let body = label.text.as_mut().unwrap();
    body.body.vert_overflow = Some(TextOverflow::Overflow);
    let mut para = body.paragraphs[0].clone();
    para.runs[0].text = "Ink".into();
    para.runs[0].props.fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(255, 0, 0))));
    body.paragraphs = vec![para; if angle == 0. { 20 } else { 40 }];
    p
}

#[test]
fn point_label_fills_cover_axis_strokes_but_keep_overlapping_tick_glyphs() {
    for kind in [ChartType::Column, ChartType::Line] {
        for angle in [0., 90.] {
            let p = rebuilt(&layer_control(kind, angle));
            let bounds = painted_bounds(&p, true).unwrap();
            let painted = render_slide(&p, 0, &RenderOpts::default());
            let mut bare = p.clone();
            let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut bare.slides[0]).shapes[0].kind else { panic!("chart") };
            chart.series[0].point_labels.clear();
            let baseline = render_slide(&bare, 0, &RenderOpts::default());
            let black = |px: &[u8]| px[0] < 40 && px[1] < 40 && px[2] < 40;
            let cyan = |px: &[u8]| px[0] < 40 && (130..210).contains(&px[1]) && (130..210).contains(&px[2]);
            let (mut ticks, mut retained_ticks, mut axes, mut retained_axes) = (0, 0, 0, 0);
            for (i, (before, after)) in baseline.pixels.chunks_exact(4).zip(painted.pixels.chunks_exact(4)).enumerate() {
                let (x, y) = (i as u32 % baseline.width, i as u32 / baseline.width);
                if x <= bounds.0 + 3 || x + 3 >= bounds.2 || y <= bounds.1 + 3 || y + 3 >= bounds.3 {
                    continue;
                }
                if black(before) {
                    ticks += 1;
                    retained_ticks += usize::from(black(after));
                }
                if cyan(before) {
                    axes += 1;
                    retained_axes += usize::from(cyan(after));
                }
            }
            assert!(
                ticks > 10 && axes > 10,
                "the control must overlap tick glyphs and axis strokes: {kind:?}, {angle}, {bounds:?}, ticks={ticks}, axes={axes}"
            );
            assert!(retained_ticks * 100 >= ticks * 95, "tick glyphs paint over the label: {kind:?}, {angle}: {retained_ticks}/{ticks}");
            assert_eq!(retained_axes, 0, "axis strokes paint below the label: {kind:?}, {angle}");
        }
    }
}

#[test]
fn axis_strokes_separate_confirmed_column_and_line_layers() {
    for kind in [ChartType::Column, ChartType::Line, ChartType::Combo] {
        let mut p = layer_control(kind, 0.);
        let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
        chart.categories = vec!["First".into(), "Middle".into(), "Last".into()];
        chart.series[0].point_labels.clear();
        chart.series[0].values = vec![Some(if kind == ChartType::Line { 0. } else { 7. }); 3];
        chart.series[0].fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(32, 80, 221))));
        chart.series[0].line = Some(Line { fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(32, 80, 221)))), width: Some(4.), ..Default::default() });
        if kind == ChartType::Combo {
            chart.series[0].kind = Some(ChartType::Column);
            let mut line = chart.series[0].clone();
            line.kind = Some(ChartType::Line);
            line.values = vec![Some(0.); 3];
            chart.series.push(line);
        }
        let image = render_slide(&rebuilt(&p), 0, &RenderOpts::default());
        let offset = ((305 * image.width + 403) * 4) as usize;
        let pixel = &image.pixels[offset..offset + 3];
        // Mixed charts retain their previous order until a native combination control exists.
        let expected = if kind == ChartType::Line { [32, 80, 221] } else { [0, 170, 170] };
        assert!(pixel.iter().zip(expected).all(|(&a, b)| a.abs_diff(b) < 5), "{kind:?}: {pixel:?}");
    }
}

#[test]
fn category_ticks_follow_the_authored_crossing_or_fixed_plot_edge_after_rebuild() {
    for (min, max, value) in [(0., 10., 7.), (-10., 0., -7.), (-10., 10., -7.)] {
        for (crossing, expected_axis, next_high) in [
            (
                ChartAxisCrossing::AutoZero,
                if min == 0. {
                    306.
                } else if max == 0. {
                    39.6
                } else {
                    172.8
                },
                false,
            ),
            (ChartAxisCrossing::Min, 306., false),
            (ChartAxisCrossing::Max, 39.6, true),
            (ChartAxisCrossing::Value(4.), 306. - (4_f64.clamp(min, max) - min) / (max - min) * 266.4, false),
            (ChartAxisCrossing::Value(20.), 39.6, false),
            (ChartAxisCrossing::Value(-20.), 306., false),
        ] {
            for position in [ChartTickPosition::NextTo, ChartTickPosition::Low, ChartTickPosition::High] {
                let mut p = layer_control(ChartType::Column, 0.);
                let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
                chart.series[0].point_labels.clear();
                chart.series[0].values = vec![Some(value)];
                chart.categories = vec!["Axis".into()];
                chart.y_min = Some(min);
                chart.y_max = Some(max);
                chart.cat_crossing = Some(crossing);
                chart.cat_label_position = Some(position);
                chart.text_styles.get_mut(&ChartTextRole::CategoryAxis).unwrap().fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(0, 170, 0))));
                let p = rebuilt(&p);
                let ShapeKind::Chart(chart) = &p.slides[0].shapes[0].kind else { panic!("chart") };
                assert_eq!(chart.cat_crossing, Some(crossing));
                assert_eq!(chart.cat_label_position, Some(position));
                let image = render_slide(&p, 0, &RenderOpts::default());
                let ys: Vec<_> = image
                    .pixels
                    .chunks_exact(4)
                    .enumerate()
                    .filter_map(|(i, px)| (px[1] > px[0].saturating_add(30) && px[1] > px[2].saturating_add(30)).then_some(i as u32 / image.width))
                    .collect();
                assert!(ys.len() > 50, "category text remains visible: {min}..{max} {crossing:?} {position:?}: {} pixels", ys.len());
                let high = position == ChartTickPosition::High || (position == ChartTickPosition::NextTo && next_high);
                let axis = match position {
                    ChartTickPosition::Low => 306.,
                    ChartTickPosition::High => 39.6,
                    ChartTickPosition::NextTo => expected_axis,
                };
                let top = f64::from(*ys.iter().min().unwrap());
                let bottom = f64::from(*ys.iter().max().unwrap());
                assert!(
                    if high { bottom < axis && top > axis - 40. } else { top > axis && bottom < axis + 40. },
                    "{min}..{max} {crossing:?} {position:?}: {top}..{bottom}"
                );
                let i = ((expected_axis.round() as u32 * image.width + 200) * 4) as usize;
                let px = &image.pixels[i..i + 3];
                assert!(px[0] < 10 && (160..180).contains(&px[1]) && (160..180).contains(&px[2]), "axis follows crossing: {crossing:?} {px:?}");
            }
        }
    }
}

#[test]
fn an_oversized_automatic_label_clips_its_marker_to_the_centered_height_budget() {
    let mut images = Vec::new();
    for policy in [TextOverflow::Ellipsis, TextOverflow::Clip, TextOverflow::Overflow] {
        let mut p = baseline_control(7., 52.);
        let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
        shape.xfrm = Some(Xfrm::new(0., 0., 820., 104.));
        let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
        let body = chart.series[0].point_labels.get_mut(&0).unwrap().text.as_mut().unwrap();
        body.body.rot = Some(29.);
        body.body.vert_overflow = Some(policy);
        body.paragraphs.truncate(2);
        for (i, p) in body.paragraphs.iter_mut().enumerate() {
            p.runs[0].text = if i == 0 { "T" } else { "z" }.into();
            p.runs[0].props.font = Some("Lato".into());
            p.runs[0].props.size = Some(if i == 0 { 52. } else { 14. });
            p.runs[0].props.fill = Some(Fill::solid(ColorRef::rgb(if i == 0 { Rgba::rgb(255, 0, 0) } else { Rgba::rgb(0, 0, 255) })));
        }
        let back = rebuilt(&p);
        let image = render_slide(&back, 0, &RenderOpts::default());
        let blue = image.pixels.chunks_exact(4).filter(|px| px[0] < 20 && px[1] < 20 && px[2] > 220).count();
        assert_eq!(blue == 0, policy != TextOverflow::Overflow, "the retained first line's background survives but the low marker is clipped");
        images.push(image.pixels);
        let ShapeKind::Chart(chart) = &back.slides[0].shapes[0].kind else { panic!("chart") };
        assert_eq!(chart.series[0].point_labels[&0].text.as_ref().unwrap().text(), "T\nz");
    }
    assert_eq!(images[0], images[1], "the marker is emitted outside the centered strip and contributes no visible ink");
}

#[test]
fn automatic_axis_gutters_follow_category_label_placement() {
    for (min, max, value) in [(0., 10., 7.), (-10., 0., -7.), (-10., 10., -7.)] {
        for position in [ChartTickPosition::NextTo, ChartTickPosition::Low, ChartTickPosition::High] {
            let mut p = layer_control(ChartType::Column, 0.);
            let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
            chart.format.plot_layout = None;
            chart.series[0].point_labels.clear();
            chart.series[0].values = vec![Some(value)];
            chart.y_min = Some(min);
            chart.y_max = Some(max);
            chart.cat_label_position = Some(position);
            let image = render_slide(&rebuilt(&p), 0, &RenderOpts::default());
            let points: Vec<_> = image
                .pixels
                .chunks_exact(4)
                .enumerate()
                .filter_map(|(i, px)| {
                    (px[0] < 10 && (160..180).contains(&px[1]) && (160..180).contains(&px[2]))
                        .then_some((i as u32 % image.width, i as u32 / image.width))
                })
                .collect();
            // The horizontal stroke begins at the vertical stroke's center, so
            // it cannot inflate the vertical range at that stroke's left edge.
            let left = points.iter().map(|p| p.0).min().unwrap();
            let top = f64::from(points.iter().filter(|p| p.0 == left).map(|p| p.1).min().unwrap());
            let bottom = f64::from(points.iter().filter(|p| p.0 == left).map(|p| p.1).max().unwrap());
            let expected_top = if position == ChartTickPosition::High { 36.1932 } else { 14.68 };
            let expected_bottom =
                if position == ChartTickPosition::Low || (position == ChartTickPosition::NextTo && min == 0.) { 323.8068 } else { 345.32 };
            assert!((top - expected_top).abs() < 2. && (bottom - expected_bottom).abs() < 2., "{min}..{max} {position:?}: {top}..{bottom}");
        }
    }
}

#[test]
fn axis_tick_margins_survive_rebuild_without_moving_tick_ink() {
    for position in [ChartTickPosition::NextTo, ChartTickPosition::High] {
        let mut reference = None;
        for margins in
            [[0., 0., 0., 0.], [10., 10., 10., 10.], [0., 0., 10., 0.], [0., 0., 0., 10.], [10., 0., 0., 0.], [0., 10., 0., 0.], [0., 0., 20., 2.]]
        {
            let mut p = layer_control(ChartType::Column, 0.);
            let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
            chart.series[0].point_labels.clear();
            chart.cat_label_position = Some(position);
            for role in [ChartTextRole::CategoryAxis, ChartTextRole::ValueAxis] {
                let body = chart.format.bodies.entry(role).or_default();
                body.inset_l = Some(margins[0]);
                body.inset_r = Some(margins[1]);
                body.inset_t = Some(margins[2]);
                body.inset_b = Some(margins[3]);
            }
            let p = rebuilt(&p);
            let pixels = render_slide(&p, 0, &RenderOpts::default()).pixels;
            if let Some(before) = &reference {
                assert!(pixels == *before, "tick margins do not offset ink: {position:?}, {margins:?}");
            } else {
                reference = Some(pixels);
            }
            let ShapeKind::Chart(chart) = &p.slides[0].shapes[0].kind else { panic!("chart") };
            for role in [ChartTextRole::CategoryAxis, ChartTextRole::ValueAxis] {
                let body = &chart.format.bodies[&role];
                assert_eq!([body.inset_l, body.inset_r, body.inset_t, body.inset_b], margins.map(Some));
            }
        }
    }
}

#[test]
fn value_tick_clearance_scales_with_its_own_font_metrics() {
    // Native advance-end clearances from independent manual/automatic plot
    // controls. Ink can stop earlier because of the final glyph's bearing.
    for (font, size, expected) in [("Inter", 16., 14.8456), ("Inter", 48., 44.529), ("Open Sans", 20., 20.7375), ("Open Sans", 40., 41.479)] {
        let mut p = layer_control(ChartType::Column, 0.);
        let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
        chart.series[0].point_labels.clear();
        chart.cat_hidden = true;
        chart.text_styles.insert(
            ChartTextRole::ValueAxis,
            RunProps {
                font: Some(font.into()),
                size: Some(size),
                fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(0, 170, 0)))),
                ..Default::default()
            },
        );
        let image = render_slide(&rebuilt(&p), 0, &RenderOpts::default());
        let right = image
            .pixels
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(i, px)| (px[1] > px[0].saturating_add(30) && px[1] > px[2].saturating_add(30)).then_some(i as u32 % image.width))
            .max()
            .expect("value tick ink");
        let gap = 100.8 - f64::from(right);
        assert!(gap >= expected - 1. && gap < expected + size * 0.15 + 1., "{font} {size}: tick clearance {gap}");
    }
}

#[test]
fn automatic_value_tick_alignment_survives_wide_chart_frames() {
    for width in [640., 960., 1280.] {
        let mut p = layer_control(ChartType::Column, 0.);
        let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
        shape.xfrm = Some(Xfrm::new(0., 0., width, 500.));
        let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
        chart.format.plot_layout = None;
        chart.series[0].point_labels.clear();
        chart.cat_hidden = true;
        chart.text_styles.get_mut(&ChartTextRole::ValueAxis).unwrap().fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(0, 170, 0))));
        let image = render_slide(&rebuilt(&p), 0, &RenderOpts::default());
        let mut ink_right = None;
        let mut axis_left = None;
        for (i, px) in image.pixels.chunks_exact(4).enumerate() {
            let x = i as u32 % image.width;
            if px[1] > px[0].saturating_add(30) && px[1] > px[2].saturating_add(30) {
                ink_right = Some(ink_right.map_or(x, |v: u32| v.max(x)));
            }
            if px[0] < 10 && (160..180).contains(&px[1]) && (160..180).contains(&px[2]) {
                axis_left = Some(axis_left.map_or(x, |v: u32| v.min(x)));
            }
        }
        // The 4pt axis stroke's center is 2pt from its left ink edge.
        let gap = f64::from(axis_left.expect("axis stroke")) + 2. - f64::from(ink_right.expect("value ticks"));
        assert!((gap - 14.8456).abs() < 3., "wide chart {width} preserves native tick clearance: {gap}");
    }
}

fn tick_frame_control(x: f64, y: f64) -> Presentation {
    let mut p = layer_control(ChartType::Column, 0.);
    let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
    shape.xfrm = Some(Xfrm::new(x, y, 440., 100.));
    let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
    chart.format.plot_layout =
        Some(ChartLayout { x: Some(0.2), y: Some(0.15), w: Some(0.7), h: Some(0.65), x_edge: true, y_edge: true, inner: true, ..Default::default() });
    chart.text_styles.get_mut(&ChartTextRole::ValueAxis).unwrap().fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(0, 153, 0))));
    p
}

#[test]
fn tick_frame_clipping_follows_the_transformed_chart_boundary() {
    for (x, y) in [(0., 0.), (40., 60.)] {
        let mut p = tick_frame_control(x, y);
        let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
        chart.series[0].point_labels.clear();
        let image = render_slide(&rebuilt(&p), 0, &RenderOpts::default());
        let mut inside = 0;
        let mut outside = 0;
        for (i, px) in image.pixels.chunks_exact(4).enumerate() {
            let green = px[1] > px[0].saturating_add(30) && px[1] > px[2].saturating_add(30);
            let black = px[..3].iter().all(|v| *v < 70);
            if green || black {
                let (ix, iy) = (f64::from(i as u32 % image.width), f64::from(i as u32 / image.width));
                if ix >= x && ix < x + 440. && iy >= y && iy < y + 100. {
                    inside += 1;
                } else {
                    outside += 1;
                }
            }
        }
        assert!(inside > 100, "axis text remains visible inside the chart");
        assert_eq!(outside, 0, "axis text does not escape the translated chart frame: {x}, {y}");
    }
}

#[test]
fn tick_frame_clipping_preserves_authored_data_label_overflow() {
    let mut p = tick_frame_control(40., 60.);
    let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
    let label = chart.series[0].point_labels.get_mut(&0).unwrap();
    label.layout = Some(ChartLayout { x: Some(0.5), y: Some(0.5), w: Some(0.2), h: Some(0.2), ..Default::default() });
    let body = label.text.as_mut().unwrap();
    body.body.vert_overflow = Some(TextOverflow::Overflow);
    body.body.rot = Some(29.);
    body.paragraphs.truncate(8);
    for p in &mut body.paragraphs {
        p.runs[0].text = "Overflow".into();
        p.runs[0].props.font = Some("Inter".into());
        p.runs[0].props.size = Some(21.);
        p.runs[0].props.fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(204, 48, 48))));
    }
    let p = rebuilt(&p);
    let yellow = painted_bounds(&p, true).expect("label background");
    let image = render_slide(&p, 0, &RenderOpts::default());
    let overflow = image
        .pixels
        .chunks_exact(4)
        .enumerate()
        .filter(|(i, px)| {
            let (x, y) = (*i as u32 % image.width, *i as u32 / image.width);
            px[0] > 130 && px[1] < 100 && px[2] < 100 && (x < yellow.0 || x > yellow.2 || y < yellow.1 || y > yellow.3)
        })
        .count();
    assert!(overflow > 100, "explicit data-label overflow still paints outside its own background");
}

fn tick_color_control(style: u32, remap: bool, root: Option<Fill>, category: Option<Fill>) -> Presentation {
    use deckcraft_color::SchemeSlot;
    let mut p = layer_control(ChartType::Column, 0.);
    let master = Arc::make_mut(&mut p.masters[0]);
    master.theme.colors.set(SchemeSlot::Dk1, Rgba::rgb(34, 68, 102));
    master.theme.colors.set(SchemeSlot::Lt1, Rgba::rgb(238, 221, 136));
    if remap {
        master.color_map = vec![("tx1".into(), "lt1".into()), ("bg1".into(), "dk1".into())];
    }
    let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
    chart.style = style;
    chart.chart_fill = Some(Fill::solid(ColorRef::rgb(Rgba::WHITE)));
    chart.series[0].point_labels.clear();
    chart.text_color = None;
    chart.text_styles.get_mut(&ChartTextRole::Default).unwrap().fill = root;
    chart.text_styles.get_mut(&ChartTextRole::CategoryAxis).unwrap().fill = category;
    chart.text_styles.get_mut(&ChartTextRole::ValueAxis).unwrap().fill = None;
    rebuilt(&p)
}

fn count_tick_color(p: &Presentation, category: bool, color: Rgba) -> usize {
    let image = render_slide(p, 0, &RenderOpts::default());
    image
        .pixels
        .chunks_exact(4)
        .enumerate()
        .filter(|(i, px)| {
            let (x, y) = (*i as u32 % image.width, *i as u32 / image.width);
            let region = if category { (380..430).contains(&x) && (307..345).contains(&y) } else { x < 98 && (40..300).contains(&y) };
            region && px[0] == color.r && px[1] == color.g && px[2] == color.b
        })
        .count()
}

#[test]
fn axis_tick_fallback_uses_chart_style_and_color_mapping() {
    for style in [1, 2, 40, 41, 48] {
        for remap in [false, true] {
            let p = tick_color_control(style, remap, None, None);
            let expected = if remap || style >= 41 { Rgba::rgb(238, 221, 136) } else { Rgba::rgb(34, 68, 102) };
            assert!(count_tick_color(&p, true, expected) > 0, "category style {style}, remap {remap}");
            let value_pixels = count_tick_color(&p, false, expected);
            assert!(value_pixels > 0, "value style {style}, remap {remap}, pixels {value_pixels}");
        }
    }
}

#[test]
fn axis_tick_color_inherits_only_from_its_own_role_and_chart() {
    let red = Rgba::rgb(204, 48, 48);
    let blue = Rgba::rgb(0, 102, 204);
    for root in [None, Some(Fill::solid(ColorRef::rgb(blue)))] {
        let expected = if root.is_some() { blue } else { Rgba::rgb(34, 68, 102) };
        let p = tick_color_control(2, false, root, Some(Fill::solid(ColorRef::rgb(red))));
        assert!(count_tick_color(&p, true, red) > 0);
        let value_pixels = count_tick_color(&p, false, expected);
        assert!(value_pixels > 0, "category fill must not color value ticks, expected pixels {value_pixels}");
        assert_eq!(count_tick_color(&p, false, red), 0);
    }
    let p = tick_color_control(2, false, Some(Fill::None), None);
    assert_eq!(count_tick_color(&p, true, Rgba::rgb(34, 68, 102)), 0);
    assert_eq!(count_tick_color(&p, false, Rgba::rgb(34, 68, 102)), 0);
}

#[test]
fn manual_inner_plot_fits_the_value_tick_reserve_and_opposite_edge() {
    for (font, size, width, x, extent, negative, hidden, expected) in [
        ("Inter", 16., 300., 0., 0.7, false, false, (32.9516, 242.9516)),
        ("Open Sans", 20., 400., 0.05, 0.7, false, false, (45.1125, 325.1125)),
        ("Inter", 16., 300., 0., 0.7, true, false, (40.3116, 250.3116)),
        ("Inter", 16., 300., 0., 0.7, false, true, (0., 210.)),
        ("Inter", 16., 300., 0., 0.99, false, false, (32.9516, 300.)),
        ("Inter", 16., 300., 0.8, 0.7, false, false, (90., 300.)),
    ] {
        let mut p = layer_control(ChartType::Column, 0.);
        let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
        shape.xfrm = Some(Xfrm::new(0., 0., width, 360.));
        let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
        chart.series[0].point_labels.clear();
        chart.series[0].values = vec![Some(if negative { -7. } else { 7. })];
        chart.y_min = Some(if negative { -10. } else { 0. });
        chart.y_max = Some(if negative { 0. } else { 10. });
        chart.y_hidden = hidden;
        chart.format.plot_layout = Some(ChartLayout {
            x: Some(x),
            y: Some(0.15),
            w: Some(extent),
            h: Some(0.65),
            x_edge: true,
            y_edge: true,
            inner: true,
            ..Default::default()
        });
        let props = chart.text_styles.get_mut(&ChartTextRole::ValueAxis).unwrap();
        props.font = Some(font.into());
        props.size = Some(size);
        let image = render_slide(&rebuilt(&p), 0, &RenderOpts::default());
        let positions: Vec<_> = image
            .pixels
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(i, px)| (px[0] < 10 && (160..180).contains(&px[1]) && (160..180).contains(&px[2])).then_some(i as u32 % image.width))
            .collect();
        let left = f64::from(*positions.iter().min().expect("axis stroke")) + if hidden { 0. } else { 2. };
        let right = f64::from(*positions.iter().max().expect("axis stroke")) + 1.;
        assert!((left - expected.0).abs() <= 1., "{font} {size}, x={x}, negative={negative}, hidden={hidden}: left {left}");
        assert!((right - expected.1).abs() <= 1., "{font} {size}, x={x}: right {right}");
    }
}

#[test]
fn manual_inner_plot_reserves_only_the_authored_category_edge() {
    for (lo, hi, value, position, y, height, expected) in [
        (0., 10., 7., ChartTickPosition::NextTo, 0., 0.99, (9.68, 68.8068)),
        (-10., 0., -7., ChartTickPosition::NextTo, 0., 0.99, (9.68, 90.32)),
        (-10., 10., -7., ChartTickPosition::NextTo, 0., 0.99, (9.68, 90.32)),
        (0., 10., 7., ChartTickPosition::High, 0., 0.99, (31.1932, 90.32)),
        (0., 10., 7., ChartTickPosition::NextTo, 0.15, 0.65, (15., 80.)),
        (0., 10., 7., ChartTickPosition::NextTo, 0.35, 0.4, (35., 75.)),
    ] {
        let mut p = tick_frame_control(0., 0.);
        let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
        chart.series[0].point_labels.clear();
        chart.series[0].values = vec![Some(value)];
        chart.y_min = Some(lo);
        chart.y_max = Some(hi);
        chart.cat_label_position = Some(position);
        let layout = chart.format.plot_layout.as_mut().unwrap();
        layout.y = Some(y);
        layout.h = Some(height);
        let image = render_slide(&rebuilt(&p), 0, &RenderOpts::default());
        let positions: Vec<_> = image
            .pixels
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(i, px)| (px[0] < 10 && (160..180).contains(&px[1]) && (160..180).contains(&px[2])).then_some(i as u32 / image.width))
            .collect();
        let top = f64::from(*positions.iter().min().expect("axis stroke")) + if hi == 0. { 2. } else { 0. };
        let bottom = f64::from(*positions.iter().max().expect("axis stroke")) + if lo == 0. { -1. } else { 1. };
        assert!((top - expected.0).abs() <= 1., "{lo}..{hi}, {position:?}: top {top}");
        assert!((bottom - expected.1).abs() <= 1., "{lo}..{hi}, {position:?}: bottom {bottom}");
    }
}

#[test]
fn cramped_columns_preserve_plot_space_between_font_reserves() {
    for (font, category_size, value_size, height, high, outer, spacing, offset, expected) in [
        ("Inter", 16., 16., 40., false, false, None, 100, (14.68, 17.8468)),
        ("Inter", 16., 16., 40., true, false, None, 100, (22.1532, 25.32)),
        ("Inter", 12., 12., 30., false, false, None, 100, (12.26, 13.0101)),
        ("Lato", 18., 18., 40., false, false, None, 100, (15.8, 17.4567)),
        ("Inter", 16., 16., 69., false, false, None, 100, (14.68, 32.8069)),
        ("Inter", 16., 32., 60., false, false, None, 100, (24.36, 35.64)),
        ("Open Sans", 12., 24., 50., false, false, None, 100, (21.34, 28.66)),
        ("Inter", 12., 12., 50., false, false, Some(48.), 100, (12.26, 23.0101)),
        ("Inter", 16., 16., 60., false, false, None, 300, (14.68, 18.1669)),
        ("Inter", 16., 16., 100., false, true, None, 100, (39.68, 47.8469)),
        ("Inter", 16., 16., 100., true, true, None, 100, (52.1532, 60.32)),
    ] {
        let mut p = layer_control(ChartType::Column, 0.);
        let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
        shape.xfrm = Some(Xfrm::new(0., 0., 700., height));
        let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
        chart.series[0].point_labels.clear();
        chart.y_min = Some(0.);
        chart.cat_label_position = Some(if high { ChartTickPosition::High } else { ChartTickPosition::NextTo });
        chart.cat_label_offset = Some(offset);
        chart.format.plot_layout = outer.then_some(ChartLayout {
            x: Some(0.1),
            y: Some(0.3),
            w: Some(0.7),
            h: Some(0.4),
            x_edge: true,
            y_edge: true,
            inner: false,
            ..Default::default()
        });
        for (role, size) in [(ChartTextRole::CategoryAxis, category_size), (ChartTextRole::ValueAxis, value_size)] {
            let props = chart.text_styles.get_mut(&role).unwrap();
            props.font = Some(font.into());
            props.size = Some(size);
        }
        if let Some(points) = spacing {
            chart.format.line_spacings.insert(ChartTextRole::CategoryAxis, deckcraft_model::text::Spacing::Pts(points));
        }
        let image = render_slide(&rebuilt(&p), 0, &RenderOpts::default());
        let positions: Vec<_> = image
            .pixels
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(i, px)| (px[0] < 10 && (160..180).contains(&px[1]) && (160..180).contains(&px[2])).then_some(i as u32 / image.width))
            .collect();
        let top = f64::from(*positions.iter().min().expect("axis stroke"));
        let bottom = f64::from(*positions.iter().max().expect("axis stroke")) - 1.;
        let stroke_top = f64::min(expected.0, expected.1 - 2.);
        assert!((top - stroke_top).abs() <= 1., "{font}, category {category_size}, value {value_size}, height {height}, high {high}: top {top}");
        assert!(
            (bottom - expected.1).abs() <= 1.,
            "{font}, category {category_size}, value {value_size}, height {height}, high {high}: bottom {bottom}"
        );
    }
}

#[test]
fn partial_inner_plot_coordinates_keep_their_independent_extents() {
    // Each subset is an original native control. Missing width/height fills
    // available space; an absent x also leaves the vertical origin automatic.
    for (fields, expected) in [
        ("", [37.9516, 14.68, 689., 263.8068]),
        ("x", [32.9516, 9.68, 700., 268.8068]),
        ("y", [32.9516, 9.68, 700., 268.8068]),
        ("w", [32.9516, 9.68, 452.9516, 268.8068]),
        ("h", [32.9516, 9.68, 700., 189.68]),
        ("xy", [32.9516, 9.68, 700., 268.8068]),
        ("xw", [140., 9.68, 560., 268.8068]),
        ("xh", [32.9516, 9.68, 700., 189.68]),
        ("yw", [32.9516, 9.68, 452.9516, 268.8068]),
        ("yh", [32.9516, 9.68, 700., 189.68]),
        ("wh", [32.9516, 9.68, 452.9516, 189.68]),
        ("xyw", [140., 9.68, 560., 268.8068]),
        ("xyh", [32.9516, 60., 700., 240.]),
        ("xwh", [140., 9.68, 560., 189.68]),
        ("ywh", [32.9516, 9.68, 452.9516, 189.68]),
        ("xywh", [140., 60., 560., 240.]),
    ] {
        let mut p = layer_control(ChartType::Column, 0.);
        let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
        shape.xfrm = Some(Xfrm::new(0., 0., 700., 300.));
        let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
        chart.series[0].point_labels.clear();
        chart.series[0].values = vec![Some(7.)];
        chart.y_min = Some(0.);
        chart.format.plot_layout = Some(ChartLayout {
            inner: true,
            x: fields.contains('x').then_some(0.2),
            y: fields.contains('y').then_some(0.2),
            w: fields.contains('w').then_some(0.6),
            h: fields.contains('h').then_some(0.6),
            x_edge: true,
            y_edge: true,
            ..Default::default()
        });
        let image = render_slide(&rebuilt(&p), 0, &RenderOpts::default());
        let pixels: Vec<_> = image
            .pixels
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(i, px)| {
                (px[0] < 10 && (160..180).contains(&px[1]) && (160..180).contains(&px[2])).then_some((i as u32 % image.width, i as u32 / image.width))
            })
            .collect();
        let actual = [
            f64::from(pixels.iter().map(|p| p.0).min().unwrap()) + 2.,
            f64::from(pixels.iter().map(|p| p.1).min().unwrap()),
            f64::from(pixels.iter().map(|p| p.0).max().unwrap()) + 1.,
            f64::from(pixels.iter().map(|p| p.1).max().unwrap()) - 1.,
        ];
        for (got, want) in actual.into_iter().zip(expected) {
            assert!((got - want).abs() <= 1., "fields {fields}: {actual:?}, expected {expected:?}");
        }
    }
}

#[test]
fn light_chart_styles_pair_tick_color_with_the_default_dark_background() {
    for (style, authored, expected) in [
        (2, None, Rgba::rgb(238, 221, 136)),
        (41, None, Rgba::rgb(34, 68, 102)),
        (48, None, Rgba::rgb(34, 68, 102)),
        (48, Some(Fill::None), Rgba::rgb(238, 221, 136)),
        (48, Some(Fill::solid(ColorRef::rgb(Rgba::rgb(204, 51, 136)))), Rgba::rgb(204, 51, 136)),
    ] {
        let mut p = tick_color_control(style, false, None, None);
        let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
        chart.chart_fill = authored;
        let p = rebuilt(&p);
        let image = render_slide(&p, 0, &RenderOpts::default());
        let pixel = (10 * image.width as usize + 10) * 4;
        assert_eq!(&image.pixels[pixel..pixel + 3], &[expected.r, expected.g, expected.b], "style {style}");
        let tick = if style >= 41 { Rgba::rgb(238, 221, 136) } else { Rgba::rgb(34, 68, 102) };
        if tick != expected {
            assert!(count_tick_color(&p, true, tick) > 0);
            assert!(count_tick_color(&p, false, tick) > 0);
        }
    }
}

#[test]
fn factor_inner_plot_positions_center_authored_extents() {
    // Independent original controls include mixed modes, negative factor
    // offsets and paired manual/automatic widths at the category reserve.
    for (fields, x_edge, y_edge, coordinates, expected) in [
        ("xywh", false, false, [0., 0., 0.4, 0.4], [180., 90., 420., 210.000076]),
        ("xywh", false, false, [0.1, 0.1, 0.4, 0.4], [240., 120., 480., 240.000076]),
        ("xywh", false, false, [-0.1, -0.1, 0.4, 0.4], [120., 60., 360., 180.000076]),
        ("xywh", false, true, [0.1, 0.1, 0.4, 0.4], [240., 30., 480., 150.000076]),
        ("xywh", true, false, [0.1, 0.1, 0.4, 0.4], [60., 120., 300., 240.000076]),
        ("ywh", false, false, [0.1, 0.1, 0.4, 0.4], [32.951649, 90., 272.95166, 210.000076]),
        ("xyh", false, false, [0.1, 0.1, 0.4, 0.4], [32.951649, 120., 600., 240.000076]),
        ("xywh", false, false, [0.1, 0.3, 0.4, 0.4], [240., 148.806824, 480., 268.806915]),
        ("xywh", true, true, [0.2, 0.5, 0.6, 0.4], [120., 150., 480., 270.000092]),
        ("xyh", true, true, [0.2, 0.5, 0.6, 0.4], [32.951649, 150.000061, 600., 268.806915]),
        ("xywh", true, true, [0.2, 0.65, 0.6, 0.25], [120., 195., 480., 270.000092]),
        ("xyh", true, true, [0.2, 0.65, 0.6, 0.25], [32.951649, 195.000061, 600., 268.806915]),
        ("xywh", true, true, [0.2, 0.02, 0.4, 0.4], [120., 9.679993, 360., 129.680069]),
        ("xyh", true, true, [0.2, 0.02, 0.4, 0.4], [32.951649, 9.679993, 600., 129.680069]),
        ("xywh", true, true, [0.2, 0.8, 0.4, 0.4], [120., 148.806824, 360., 268.806915]),
        ("xyh", true, true, [0.2, 0.8, 0.4, 0.4], [32.951649, 148.806824, 600., 268.806915]),
        ("ywh", true, true, [0.1, 0.1, 0.4, 0.95], [32.951649, 9.680054, 272.95166, 268.806915]),
        ("xwh", true, true, [0.1, 0.1, 0.4, 0.9], [60., 9.680054, 300., 268.806915]),
        ("ywh", false, false, [0.1, 0.1, 0.4, 0.9], [32.951649, 15.000046, 272.95166, 268.806915]),
        ("xwh", false, false, [0.1, 0.1, 0.4, 0.9], [240., 9.680054, 480., 268.806915]),
    ] {
        let mut p = layer_control(ChartType::Column, 0.);
        let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
        shape.xfrm = Some(Xfrm::new(0., 0., 600., 300.));
        let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
        chart.series[0].point_labels.clear();
        chart.series[0].values = vec![Some(7.)];
        chart.y_min = Some(0.);
        chart.format.plot_layout = Some(ChartLayout {
            inner: true,
            x: fields.contains('x').then_some(coordinates[0]),
            y: fields.contains('y').then_some(coordinates[1]),
            w: fields.contains('w').then_some(coordinates[2]),
            h: fields.contains('h').then_some(coordinates[3]),
            x_edge,
            y_edge,
            ..Default::default()
        });
        let image = render_slide(&rebuilt(&p), 0, &RenderOpts::default());
        let pixels: Vec<_> = image
            .pixels
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(i, px)| {
                (px[0] < 10 && (160..180).contains(&px[1]) && (160..180).contains(&px[2])).then_some((i as u32 % image.width, i as u32 / image.width))
            })
            .collect();
        let actual = [
            f64::from(pixels.iter().map(|p| p.0).min().unwrap()) + 2.,
            f64::from(pixels.iter().map(|p| p.1).min().unwrap()),
            f64::from(pixels.iter().map(|p| p.0).max().unwrap()) + 1.,
            f64::from(pixels.iter().map(|p| p.1).max().unwrap()) - 1.,
        ];
        for (got, want) in actual.into_iter().zip(expected) {
            assert!(
                (got - want).abs() <= 1.,
                "fields {fields}, edge {x_edge}/{y_edge}, coordinates {coordinates:?}: {actual:?}, expected {expected:?}"
            );
        }
    }
}

#[test]
fn hidden_value_ticks_preserve_axis_strokes_and_rebuild_separately_from_deletion() {
    for automatic in [false, true] {
        for (deleted, labels_hidden) in [(false, false), (false, true), (true, false), (true, true)] {
            let mut p = layer_control(ChartType::Column, 0.);
            let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
            shape.xfrm = Some(Xfrm::new(0., 0., 640., 280.));
            let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
            chart.series[0].point_labels.clear();
            chart.series[0].values = vec![Some(7.)];
            chart.y_min = Some(0.);
            chart.y_hidden = deleted;
            chart.y_tick_labels_hidden = labels_hidden;
            chart.format.plot_layout = (!automatic).then_some(ChartLayout {
                inner: true,
                x: Some(0.15),
                y: Some(0.15),
                w: Some(0.7),
                h: Some(0.6),
                x_edge: true,
                y_edge: true,
                ..Default::default()
            });
            for role in [ChartTextRole::CategoryAxis, ChartTextRole::ValueAxis] {
                let props = chart.text_styles.get_mut(&role).unwrap();
                props.size = Some(12.);
                props.fill = Some(Fill::solid(ColorRef::rgb(if role == ChartTextRole::ValueAxis { Rgba::rgb(0, 153, 0) } else { Rgba::BLACK })));
            }
            let back = rebuilt(&p);
            let ShapeKind::Chart(chart) = &back.slides[0].shapes[0].kind else { panic!("chart") };
            assert_eq!((chart.y_hidden, chart.y_tick_labels_hidden), (deleted, labels_hidden));
            let image = render_slide(&back, 0, &RenderOpts::default());
            let value_stroke = image.pixels.chunks_exact(4).enumerate().any(|(i, px)| {
                let row = i as u32 / image.width;
                (50..180).contains(&row) && px[0] < 10 && (160..180).contains(&px[1]) && (160..180).contains(&px[2])
            });
            let value_text = image.pixels.chunks_exact(4).any(|px| px[0] < 10 && (145..160).contains(&px[1]) && px[2] < 10);
            assert_eq!(value_stroke, !deleted, "automatic {automatic}, deleted {deleted}, labels hidden {labels_hidden}");
            assert_eq!(value_text, !deleted && !labels_hidden, "automatic {automatic}, deleted {deleted}, labels hidden {labels_hidden}");
        }
    }
}

#[test]
fn category_highlights_use_authored_gap_and_individually_rounded_line_metrics() {
    // Fractional original controls distinguish rounding each vertical metric
    // from rounding their sum or measuring at the rounded painted font size.
    for (font, size, ascent, height) in [
        ("Inter", 6.75, 6.75 * 1984. / 2048., 9.),
        ("Inter", 20.2, 20.2 * 1984. / 2048., 25.),
        ("Lato", 19.75, 19.75 * 1974. / 2000., 23.),
        ("Open Sans", 14.52, 14.52 * 2189. / 2048., 20.),
    ] {
        let mut p = layer_control(ChartType::Column, 0.);
        let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
        shape.xfrm = Some(Xfrm::new(0., 0., 780., 420.));
        let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
        chart.series[0].point_labels.clear();
        chart.y_min = Some(0.);
        chart.categories = vec!["A".into()];
        chart.format.plot_layout = Some(ChartLayout {
            inner: true,
            x: Some(0.14),
            y: Some(0.25),
            w: Some(0.68),
            h: Some(0.4),
            x_edge: true,
            y_edge: true,
            ..Default::default()
        });
        chart.format.bodies.entry(ChartTextRole::CategoryAxis).or_default().anchor = Some(Anchor::Bottom);
        chart.text_styles.insert(
            ChartTextRole::CategoryAxis,
            RunProps {
                font: Some(font.into()),
                size: Some(size),
                highlight: Some(ColorRef::rgb(Rgba::rgb(255, 170, 0))),
                fill: Some(Fill::solid(ColorRef::rgb(Rgba::BLACK))),
                ..Default::default()
            },
        );
        let image = render_slide(&rebuilt(&p), 0, &RenderOpts { scale: 4., ..Default::default() });
        let rows: Vec<_> = image
            .pixels
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(i, px)| (px[0] > 250 && (165..175).contains(&px[1]) && px[2] < 5).then_some(i as u32 / image.width))
            .collect();
        let top = f64::from(*rows.iter().min().unwrap()) / 4.;
        let bottom = f64::from(*rows.iter().max().unwrap() + 1) / 4.;
        let expected_top = 273. + 2. * ascent / 3.;
        assert!((top - expected_top).abs() <= 0.25, "{font} {size}: top {top}, expected {expected_top}");
        assert!((bottom - expected_top - height).abs() <= 0.25, "{font} {size}: bottom {bottom}, height {height}");
    }
}

#[test]
fn category_glyph_origins_follow_the_page_grid_while_highlights_move_continuously() {
    let render = |phase| {
        let mut p = layer_control(ChartType::Column, 0.);
        let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
        shape.xfrm = Some(Xfrm::new(0., phase, 800., 360.));
        let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
        chart.series[0].point_labels.clear();
        chart.categories = vec!["A".into()];
        chart.y_min = Some(0.);
        chart.format.plot_layout = Some(ChartLayout {
            inner: true,
            x: Some(0.12),
            y: Some(0.11),
            w: Some(0.72),
            h: Some(0.74),
            x_edge: true,
            y_edge: true,
            ..Default::default()
        });
        chart.text_styles.insert(
            ChartTextRole::CategoryAxis,
            RunProps {
                font: Some("Lato".into()),
                size: Some(16.),
                highlight: Some(ColorRef::rgb(Rgba::rgb(255, 170, 0))),
                fill: Some(Fill::solid(ColorRef::rgb(Rgba::BLACK))),
                ..Default::default()
            },
        );
        chart.text_styles.get_mut(&ChartTextRole::ValueAxis).unwrap().fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(0, 153, 0))));
        render_slide(&rebuilt(&p), 0, &RenderOpts { scale: 4., ..Default::default() })
    };
    let before = render(0.);
    let after = render(0.12);
    let black =
        |image: &deckcraft_engine::render::Image| image.pixels.chunks_exact(4).map(|px| px[0] < 40 && px[1] < 40 && px[2] < 40).collect::<Vec<_>>();
    let ink = black(&before);
    assert!(ink.iter().filter(|p| **p).count() > 30);
    assert_eq!(ink, black(&after), "both continuous baselines paint at page Y=332.64pt");
    // The highlight starts at 316.528pt and moves by the authored 0.12pt.
    // Its top edge is above the A, so this pixel observes only that geometry.
    let edge = ((316.5 * 4.) as usize * before.width as usize + 384 * 4) * 4;
    assert_ne!(&before.pixels[edge..edge + 4], &after.pixels[edge..edge + 4]);
}

#[test]
fn grouped_charts_keep_font_and_stroke_sizes_in_slide_points() {
    for (font, size, sx, sy) in [("Inter", 14.25, 1.2, 1.2), ("Lato", 18.3, 1.35, 0.8), ("Open Sans", 11.6, 0.75, 0.75)] {
        let mut p = layer_control(ChartType::Column, 0.);
        let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
        shape.xfrm = Some(Xfrm::new(60., 50., 540. * sx, 250. * sy));
        let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
        chart.series[0].point_labels.clear();
        chart.categories = vec!["Agy".into()];
        chart.y_min = Some(0.);
        chart.format.plot_layout = Some(ChartLayout {
            inner: true,
            x: Some(0.12),
            y: Some(0.11),
            w: Some(0.72),
            h: Some(0.74),
            x_edge: true,
            y_edge: true,
            ..Default::default()
        });
        for role in [ChartTextRole::CategoryAxis, ChartTextRole::ValueAxis] {
            let props = chart.text_styles.entry(role).or_default();
            props.font = Some(font.into());
            props.size = Some(size);
        }
        let mut grouped = p.clone();
        let slide = Arc::make_mut(&mut grouped.slides[0]);
        let mut child = slide.shapes.remove(0);
        child.xfrm = Some(Xfrm::new(0., 0., 540., 250.));
        slide.shapes.push(Shape {
            id: ShapeId(200),
            xfrm: Some(Xfrm::new(60., 50., 540. * sx, 250. * sy)),
            kind: ShapeKind::Group { children: vec![child], child: Xfrm::new(0., 0., 540., 250.) },
            ..Default::default()
        });
        for scale in [1.5, 4.] {
            let opts = RenderOpts { scale, ..Default::default() };
            let plain = render_slide(&p, 0, &opts);
            let in_group = render_slide(&grouped, 0, &opts);
            assert!(plain.pixels == in_group.pixels, "{font} {size}, group scale {sx} x {sy}, render scale {scale}");
        }
    }
}

#[test]
fn chart_frames_stay_upright_at_their_transformed_centers() {
    for kind in [ChartType::Column, ChartType::Line] {
        for angle in [0., 19.] {
            let mut p = layer_control(kind, 0.);
            let slide = Arc::make_mut(&mut p.slides[0]);
            let mut child = slide.shapes.remove(0);
            let logical = Xfrm { rot: angle, ..Xfrm::new(70., 40., 630., 210.) };
            child.xfrm = Some(logical);
            let ShapeKind::Chart(chart) = &mut child.kind else { panic!("chart") };
            chart.categories = ["Agy", "Hgj", "Qyp"].map(str::to_owned).to_vec();
            chart.series[0].values = vec![Some(4.), Some(8.), Some(6.)];
            chart.series[0].point_labels.clear();
            chart.y_min = Some(0.);
            chart.format.plot_layout = Some(ChartLayout {
                inner: true,
                x: Some(0.12),
                y: Some(0.11),
                w: Some(0.72),
                h: Some(0.74),
                x_edge: true,
                y_edge: true,
                ..Default::default()
            });
            let group = Xfrm { rot: -27., ..Xfrm::new(40., 40., 1125., 400.) };
            let children = Xfrm::new(0., 0., 900., 500.);
            let transform = deckcraft_geom::group_child_affine(&group, children.rect());
            let center = transform * logical.center();
            let mut plain = child.clone();
            plain.xfrm = Some(Xfrm::new(center.x - 787.5 / 2., center.y - 168. / 2., 787.5, 168.));
            slide.shapes.push(plain);
            let mut grouped = p.clone();
            Arc::make_mut(&mut grouped.slides[0]).shapes = vec![Shape {
                id: ShapeId(200),
                xfrm: Some(group),
                kind: ShapeKind::Group { children: vec![child], child: children },
                ..Default::default()
            }];
            for scale in [1.5, 4.] {
                let opts = RenderOpts { scale, ..Default::default() };
                let a = render_slide(&p, 0, &opts);
                let b = render_slide(&grouped, 0, &opts);
                let changed = a.pixels.chunks_exact(4).zip(b.pixels.chunks_exact(4)).filter(|(a, b)| a != b).count();
                assert!(a.pixels == b.pixels, "{kind:?}, local rotation{angle}, view scale{scale}, changed{changed}, center{center:?}");
            }
        }
    }
}

#[test]
fn reflected_chart_frames_keep_asymmetric_data_upright() {
    for kind in [ChartType::Column, ChartType::Line] {
        // Own H/V, parent H/V/both, then two reflections across nested groups.
        for (own_h, own_v, parent_h, parent_v, nested) in [
            (true, false, false, false, false),
            (false, true, false, false, false),
            (false, false, true, false, false),
            (false, false, false, true, false),
            (false, false, true, true, false),
            (false, false, false, true, true),
        ] {
            let mut p = layer_control(kind, 0.);
            let slide = Arc::make_mut(&mut p.slides[0]);
            let mut child = slide.shapes.remove(0);
            let logical = if nested { Xfrm::new(95., 65., 430., 190.) } else { Xfrm::new(70., 40., 510., 210.) };
            child.xfrm = Some(Xfrm { flip_h: own_h, flip_v: own_v, ..logical });
            let ShapeKind::Chart(chart) = &mut child.kind else { panic!("chart") };
            chart.categories = ["Ax", "Bgy", "Cqpq"].map(str::to_owned).to_vec();
            chart.series[0].values = vec![Some(2.), Some(7.), Some(4.)];
            chart.series[0].point_labels.clear();
            chart.y_min = Some(0.);
            chart.format.plot_layout = Some(ChartLayout {
                inner: true,
                x: Some(0.12),
                y: Some(0.11),
                w: Some(0.72),
                h: Some(0.74),
                x_edge: true,
                y_edge: true,
                ..Default::default()
            });
            let group = Xfrm {
                rot: if nested { 19. } else { -27. },
                flip_h: parent_h,
                flip_v: parent_v,
                ..if nested { Xfrm::new(40., 40., 990., 425.) } else { Xfrm::new(40., 40., 945., 400.) }
            };
            let children = Xfrm::new(0., 0., 900., 500.);
            let mut transform = deckcraft_geom::group_child_affine(&group, children.rect());
            let mut grouped_shape = Shape {
                id: ShapeId(200),
                xfrm: Some(group),
                kind: ShapeKind::Group { children: vec![child.clone()], child: children },
                ..Default::default()
            };
            if nested {
                let outer = Xfrm { rot: -13., flip_h: true, ..Xfrm::new(0., 0., 960., 900.) };
                let outer_children = Xfrm::new(0., 0., 960., 900.);
                transform = deckcraft_geom::group_child_affine(&outer, outer_children.rect()) * transform;
                grouped_shape = Shape {
                    id: ShapeId(201),
                    xfrm: Some(outer),
                    kind: ShapeKind::Group { children: vec![grouped_shape], child: outer_children },
                    ..Default::default()
                };
            }
            let center = transform * logical.center();
            // Only the inner group scales; the reflected outer group is an isometry.
            let (width, height) = if nested { (473., 161.5) } else { (535.5, 168.) };
            child.xfrm = Some(Xfrm::new(center.x - width / 2., center.y - height / 2., width, height));
            slide.shapes.push(child);
            let mut grouped = p.clone();
            Arc::make_mut(&mut grouped.slides[0]).shapes = vec![grouped_shape];
            for scale in [1.5, 4.] {
                let opts = RenderOpts { scale, ..Default::default() };
                let expected = render_slide(&p, 0, &opts);
                let actual = render_slide(&grouped, 0, &opts);
                assert!(
                    expected.pixels == actual.pixels,
                    "{kind:?}, own({own_h},{own_v}), parent({parent_h},{parent_v}), nested{nested}, scale{scale}"
                );
            }
        }
    }
}

#[test]
fn overlapping_major_grid_lines_keep_one_authored_opacity() {
    for alpha in [64, 128] {
        let mut samples = Vec::new();
        for major in [2., 0.02] {
            let mut p = layer_control(ChartType::Column, 0.);
            let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
            shape.xfrm = Some(Xfrm::new(0., 0., 840., 420.));
            let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
            chart.series[0].point_labels.clear();
            chart.series[0].values = vec![Some(0.)];
            chart.y_min = Some(0.);
            chart.y_max = Some(10.);
            chart.y_major = Some(major);
            chart.format.plot_layout = Some(ChartLayout {
                inner: true,
                x: Some(0.2),
                y: Some(0.15),
                w: Some(0.7),
                h: Some(0.6),
                x_edge: true,
                y_edge: true,
                ..Default::default()
            });
            chart.format.lines.get_mut(&ChartLineRole::MajorGrid).unwrap().fill =
                Some(Fill::solid(ColorRef::rgb(Rgba::rgb(192, 0, 192).with_alpha(alpha))));
            let image = render_slide(&rebuilt(&p), 0, &RenderOpts { scale: 4., ..Default::default() });
            // Value4 is centered at214.2pt in both plots. This interior pixel
            // has full stroke coverage, away from marks, axes and text.
            let index = (857 * image.width as usize + 1200) * 4;
            samples.push(image.pixels[index..index + 4].to_vec());
        }
        assert_eq!(samples[0][1], 255 - alpha, "sparse grid alpha{alpha}");
        assert_eq!(samples[0], samples[1], "overlapping grid paths keep alpha{alpha}");
    }
}

#[test]
fn overlapping_major_grid_edges_keep_one_coverage() {
    for alpha in [128, 255] {
        let mut samples = Vec::new();
        for major in [2., 0.02] {
            let mut p = layer_control(ChartType::Column, 0.);
            let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
            shape.xfrm = Some(Xfrm::new(0., 0., 841., 420.));
            let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
            chart.series[0].point_labels.clear();
            chart.series[0].values = vec![Some(0.)];
            chart.y_min = Some(0.);
            chart.y_max = Some(10.);
            chart.y_major = Some(major);
            chart.format.plot_layout = Some(ChartLayout {
                inner: true,
                x: Some(0.2),
                y: Some(0.15),
                w: Some(0.7),
                h: Some(0.6),
                x_edge: true,
                y_edge: true,
                ..Default::default()
            });
            chart.format.lines.get_mut(&ChartLineRole::MajorGrid).unwrap().fill =
                Some(Fill::solid(ColorRef::rgb(Rgba::rgb(192, 0, 192).with_alpha(alpha))));
            let image = render_slide(&rebuilt(&p), 0, &RenderOpts { scale: 4., ..Default::default() });
            // At value4 the sparse stroke covers the full row, while the
            // endpoint at756.9pt covers only part of device column3027.
            // Overlapping strokes share that same physical endpoint.
            let index = (857 * image.width as usize + 3027) * 4;
            samples.push(image.pixels[index..index + 4].to_vec());
        }
        assert!(samples[0][1] > 255 - alpha && samples[0][1] < 255, "partial endpoint coverage");
        assert_eq!(samples[0], samples[1], "overlapping grid endpoints keep alpha{alpha}");
    }
}

#[test]
fn major_grid_omission_uses_the_visible_axis_and_its_unclamped_crossing() {
    for (crossing, no_fill, deleted, hidden_ticks, omitted) in [
        (None, false, false, false, Some(0.)),
        (None, true, false, false, None),
        (None, false, true, false, None),
        (None, false, false, true, Some(0.)),
        (Some(24.), false, false, false, None),
        (Some(3.), false, false, false, Some(3.)),
        (Some(2.5), false, false, false, None),
    ] {
        let mut p = layer_control(ChartType::Column, 0.);
        let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
        shape.xfrm = Some(Xfrm::new(0., 0., 840., 420.));
        let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
        chart.series[0].point_labels.clear();
        chart.series[0].values = vec![Some(0.)];
        chart.y_min = Some(-6.);
        chart.y_max = Some(18.);
        chart.y_major = Some(3.);
        chart.cat_crossing = crossing.map(ChartAxisCrossing::Value);
        chart.cat_deleted = deleted;
        chart.cat_hidden = hidden_ticks;
        let line = chart.format.lines.get_mut(&ChartLineRole::CategoryAxis).unwrap();
        line.width = Some(0.5);
        if no_fill {
            line.fill = Some(Fill::None);
        }
        chart.format.plot_layout = Some(ChartLayout {
            inner: true,
            x: Some(0.2),
            y: Some(0.15),
            w: Some(0.7),
            h: Some(0.6),
            x_edge: true,
            y_edge: true,
            ..Default::default()
        });
        let image = render_slide(&rebuilt(&p), 0, &RenderOpts { scale: 4., ..Default::default() });
        for value in [-6., -3., 0., 3., 6., 9., 12., 15., 18.] {
            // Sample the4pt grid beside the thinner axis, so axis paint cannot
            // conceal a grid that should have been omitted.
            let y = 315. - (value + 6.) / 24. * 252. + 1.25;
            let index = ((y * 4.) as usize * image.width as usize + 800) * 4;
            let expected = if omitted == Some(value) { [255, 255, 255, 255] } else { [192, 0, 192, 255] };
            assert_eq!(
                &image.pixels[index..index + 4],
                &expected,
                "crossing{crossing:?}, noFill{no_fill}, deleted{deleted}, hidden{hidden_ticks}, tick{value}"
            );
        }
    }
}

#[test]
fn category_line_spacing_moves_lines_without_stretching_their_highlights() {
    use deckcraft_model::text::Spacing;
    for (spacing, baselines) in [(None, [15., 34., 52.]), (Some(Spacing::Pct(1.4)), [19., 45., 71.]), (Some(Spacing::Pts(30.5)), [23., 54., 85.])] {
        let mut p = layer_control(ChartType::Column, 0.);
        let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
        shape.xfrm = Some(Xfrm::new(0., 0., 780., 420.));
        let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
        chart.series[0].point_labels.clear();
        chart.categories = vec!["Ag\nAg\nAg".into()];
        chart.y_min = Some(0.);
        chart.format.plot_layout = Some(ChartLayout {
            inner: true,
            x: Some(0.14),
            y: Some(0.3),
            w: Some(0.68),
            h: Some(0.4),
            x_edge: true,
            y_edge: true,
            ..Default::default()
        });
        chart.text_styles.insert(
            ChartTextRole::CategoryAxis,
            RunProps {
                font: Some("Lato".into()),
                size: Some(15.4),
                highlight: Some(ColorRef::rgb(Rgba::rgb(255, 170, 0))),
                fill: Some(Fill::solid(ColorRef::rgb(Rgba::BLACK))),
                ..Default::default()
            },
        );
        if let Some(spacing) = spacing {
            chart.format.line_spacings.insert(ChartTextRole::CategoryAxis, spacing);
        }
        let image = render_slide(&rebuilt(&p), 0, &RenderOpts { scale: 4., ..Default::default() });
        let mut painted = vec![false; image.height as usize];
        for (i, px) in image.pixels.chunks_exact(4).enumerate() {
            if px[0] > 250 && (165..175).contains(&px[1]) && px[2] < 5 {
                painted[i / image.width as usize] = true;
            }
        }
        let starts: Vec<_> =
            painted.iter().enumerate().filter_map(|(i, row)| (*row && (i == 0 || !painted[i - 1])).then_some(i as f64 / 4.)).collect();
        // The unrounded authored ascent determines the axis gap. Each line's
        // highlight stays 18pt high even when the paragraph pitch changes.
        assert_eq!(starts.len(), 3);
        for (top, baseline) in starts.into_iter().zip(baselines) {
            let expected = 294. + 2. * (15.4 * 1974. / 2000.) / 3. + baseline - 15.;
            assert!((top - expected).abs() <= 0.25, "{spacing:?}: {top}, expected {expected}");
            let center = ((top + 9.) * 4.) as usize;
            assert!(painted[center]);
        }
    }
}

#[test]
fn high_category_frames_use_continuous_percentage_height_and_rounded_exact_pitch() {
    use deckcraft_model::text::Spacing;
    // Original high-side controls compare frame motion to the same font's
    // single natural line. The absolute axis gap is independent of this rule.
    for (font, size, expected) in [
        ("Inter", 15.7, [0., -1.70001, -3.005, -37.98999, -51.09003, -59.005]),
        ("Open Sans", 11.3, [0., -1.61511, -3.61005, -30.78003, -41.625, -59.61005]),
    ] {
        let mut first_top = None;
        for (index, (lines, spacing)) in [
            (1, None),
            (1, Some(Spacing::Pct(1.3))),
            (1, Some(Spacing::Pts(27.5))),
            (3, None),
            (3, Some(Spacing::Pct(1.3))),
            (3, Some(Spacing::Pts(27.5))),
        ]
        .into_iter()
        .enumerate()
        {
            let mut p = layer_control(ChartType::Column, 0.);
            let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
            shape.xfrm = Some(Xfrm::new(0., 0., 780., 420.));
            let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
            chart.series[0].point_labels.clear();
            chart.categories = vec![vec!["Ag"; lines].join("\n")];
            chart.cat_label_position = Some(ChartTickPosition::High);
            chart.y_min = Some(0.);
            chart.format.plot_layout = Some(ChartLayout {
                inner: true,
                x: Some(0.14),
                y: Some(0.3),
                w: Some(0.68),
                h: Some(0.4),
                x_edge: true,
                y_edge: true,
                ..Default::default()
            });
            chart.text_styles.insert(
                ChartTextRole::CategoryAxis,
                RunProps {
                    font: Some(font.into()),
                    size: Some(size),
                    highlight: Some(ColorRef::rgb(Rgba::rgb(255, 170, 0))),
                    fill: Some(Fill::solid(ColorRef::rgb(Rgba::BLACK))),
                    ..Default::default()
                },
            );
            if let Some(spacing) = spacing {
                chart.format.line_spacings.insert(ChartTextRole::CategoryAxis, spacing);
            }
            let image = render_slide(&rebuilt(&p), 0, &RenderOpts { scale: 4., ..Default::default() });
            let top = image.pixels.chunks_exact(4).position(|px| px[0] > 250 && (165..175).contains(&px[1]) && px[2] < 5).expect("category highlight")
                as f64;
            let top = (top / f64::from(image.width)).floor() / 4.;
            let delta = top - *first_top.get_or_insert(top);
            assert!((delta - expected[index]).abs() <= 0.25, "{font}, {lines} lines, {spacing:?}: {delta}, expected {}", expected[index]);
        }
    }
}

#[test]
fn value_tick_reserves_measure_bounds_and_major_unit_neighbors() {
    // Original proportional-digit controls distinguish exact bound neighbors
    // from both the widest painted tick and endpoint-only measurement.
    for (lo, hi, step, format, expected) in [
        (11., 45., 1., "0", 90.2966),
        (11., 46., 1., "0", 89.2617),
        (-45., -11., 1., "0", 108.6966),
        (-46., -11., 1., "0", 107.6616),
        (11.1, 45.4, 1., "0.0", 127.6617),
        (-45.4, -11.1, 1., "0.0", 146.0616),
        (11., 45.5, 1., "0.0", 125.5517),
        (11.1, 45.4, 0.5, "0.0", 126.6267),
        (11.1, 11.4, 1., "0.0", 117.4867),
        (-11.4, -11.1, 1., "0.0", 135.8867),
        (11.1, 11.4, 10., "0.0", 108.5217),
    ] {
        for kern in [None, Some(0.), Some(12.), Some(100.)] {
            let mut p = layer_control(ChartType::Column, 0.);
            let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
            shape.xfrm = Some(Xfrm::new(0., 0., 840., 420.));
            let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
            chart.series[0].point_labels.clear();
            chart.series[0].values = vec![Some((lo + hi) / 2.)];
            chart.y_min = Some(lo);
            chart.y_max = Some(hi);
            chart.y_major = Some(step);
            chart.y_format = Some(format.into());
            chart.format.plot_layout = Some(ChartLayout {
                inner: true,
                x: Some(0.),
                y: Some(0.15),
                w: Some(0.7),
                h: Some(0.6),
                x_edge: true,
                y_edge: true,
                ..Default::default()
            });
            let props = chart.text_styles.get_mut(&ChartTextRole::ValueAxis).unwrap();
            props.font = Some("Inter".into());
            props.size = Some(40.);
            props.kern = kern;
            let image = render_slide(&rebuilt(&p), 0, &RenderOpts { scale: 4., ..Default::default() });
            let left = image
                .pixels
                .chunks_exact(4)
                .enumerate()
                .filter_map(|(i, px)| (px[0] < 10 && (160..180).contains(&px[1]) && (160..180).contains(&px[2])).then_some(i as u32 % image.width))
                .min()
                .expect("axis stroke");
            let left = f64::from(left) / 4. + 2.;
            assert!((left - expected).abs() <= 0.25, "{lo}..{hi}, major {step}, {format}, kern {kern:?}: left {left}, expected {expected}");
        }
    }
}

#[test]
fn inner_plot_keeps_first_range_right_edge_when_tick_width_changes() {
    for (values, minimum, maximum, font, expected_left, expected_right) in [
        ([57.4, 62.93], None, Some(64.1), "Inter", 112.2385, 700.8136),
        ([-62.93, -57.4], Some(-64.1), None, "Inter", 121.4385, 710.0135),
        ([57.4, 62.93], None, None, "Inter", 124.8885, 713.1735),
        ([57.4, 62.93], None, Some(64.1), "Lato", 106., 694.),
    ] {
        for fixed in [false, true] {
            let mut p = layer_control(ChartType::Column, 0.);
            let shape = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
            shape.xfrm = Some(Xfrm::new(0., 0., 840., 420.));
            let ShapeKind::Chart(chart) = &mut shape.kind else { panic!("chart") };
            chart.series[0].point_labels.clear();
            chart.series[0].values = vec![Some(values[0])];
            let mut other = chart.series[0].clone();
            other.values = vec![Some(values[1])];
            chart.series.push(other);
            chart.y_min = minimum;
            chart.y_max = maximum;
            chart.y_major = Some(0.001);
            let final_scale = chart.axis_scale(false, 5).unwrap();
            if fixed {
                chart.y_min = Some(final_scale.0);
                chart.y_max = Some(final_scale.1);
                chart.y_major = Some(final_scale.2);
            }
            assert_eq!(chart.axis_scale(false, 5), Some(final_scale));
            chart.format.plot_layout = Some(ChartLayout {
                inner: true,
                x: Some(0.),
                y: Some(0.15),
                w: Some(0.7),
                h: Some(0.6),
                x_edge: true,
                y_edge: true,
                ..Default::default()
            });
            let props = chart.text_styles.get_mut(&ChartTextRole::ValueAxis).unwrap();
            props.font = Some(font.into());
            props.size = Some(20.);
            let image = render_slide(&rebuilt(&p), 0, &RenderOpts { scale: 4., ..Default::default() });
            let positions: Vec<_> = image
                .pixels
                .chunks_exact(4)
                .enumerate()
                .filter_map(|(i, px)| (px[0] < 10 && (160..180).contains(&px[1]) && (160..180).contains(&px[2])).then_some(i as u32 % image.width))
                .collect();
            let left = f64::from(*positions.iter().min().expect("axis stroke")) / 4. + 2.;
            let right = f64::from(*positions.iter().max().expect("axis stroke")) / 4. + 0.25;
            let expected_right = if fixed { expected_left + 588. } else { expected_right };
            assert!((left - expected_left).abs() <= 0.25, "{font}, {minimum:?}..{maximum:?}, fixed {fixed}: left {left}");
            assert!(
                (right - expected_right).abs() <= 0.25,
                "{font}, {minimum:?}..{maximum:?}, fixed {fixed}: right {right}, expected {expected_right}"
            );
        }
    }
}

#[test]
fn value_tick_glyphs_ignore_authored_kerning_thresholds() {
    for format in ["0.0", "\"AV\"0.0"] {
        let mut images = Vec::new();
        for kern in [None, Some(0.), Some(12.), Some(100.)] {
            let mut p = layer_control(ChartType::Column, 0.);
            let ShapeKind::Chart(chart) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
            chart.series[0].point_labels.clear();
            chart.y_min = Some(11.1);
            chart.y_max = Some(45.5);
            chart.y_major = Some(1.);
            chart.y_format = Some(format.into());
            let props = chart.text_styles.get_mut(&ChartTextRole::ValueAxis).unwrap();
            props.size = Some(40.);
            props.kern = kern;
            let back = rebuilt(&p);
            let ShapeKind::Chart(chart) = &back.slides[0].shapes[0].kind else { panic!("chart") };
            assert_eq!(chart.text_props(ChartTextRole::ValueAxis).kern, kern);
            images.push(render_slide(&back, 0, &RenderOpts { scale: 2., ..Default::default() }).pixels);
        }
        assert!(images.windows(2).all(|pair| pair[0] == pair[1]));
    }
}
