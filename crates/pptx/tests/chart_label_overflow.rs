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
