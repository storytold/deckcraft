//! Original chart-label controls: stored text survives while automatic labels clip or
//! ellipsize whole lines. Painted colours verify the marker separately from text import.
use std::sync::Arc;

use deckcraft_color::Rgba;
use deckcraft_engine::render::{RenderOpts, render_slide};
use deckcraft_model::chart::{Chart, ChartTextRole, ChartType, DataLabel, Series};
use deckcraft_model::text::{RunProps, TextBody, TextOverflow};
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
