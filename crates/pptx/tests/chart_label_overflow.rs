//! Original chart-label controls: stored text survives while automatic labels clip or
//! ellipsize whole lines. Painted colours verify the marker separately from text import.
use std::sync::Arc;

use deckcraft_color::Rgba;
use deckcraft_engine::render::{RenderOpts, render_slide};
use deckcraft_model::chart::{Chart, ChartLayout, ChartTextRole, ChartType, DataLabel, Series};
use deckcraft_model::text::{Anchor, RunProps, TextBody, TextOverflow};
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
