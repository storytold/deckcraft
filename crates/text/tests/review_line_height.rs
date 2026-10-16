//! Independent public-API geometry oracles for minimum line pitch.
use deckcraft_geom::{Point, Rect};
use deckcraft_model::resolve::Ctx;
use deckcraft_model::text::{Anchor, AutoFit, BodyProps, LineHeight, Run, RunKind, RunProps, Spacing, TextBody};
use deckcraft_model::{Presentation, Shape};
use deckcraft_text::{NoFields, Opts, Pos, TextLayout, layout};

fn shape() -> Shape {
    let mut body = TextBody::from_text("fixture");
    body.body = BodyProps {
        inset_l: Some(0.0),
        inset_r: Some(0.0),
        inset_t: Some(0.0),
        inset_b: Some(0.0),
        anchor: Some(Anchor::Top),
        autofit: Some(AutoFit::None),
        wrap: Some(false),
        ..Default::default()
    };
    let p = &mut body.paragraphs[0];
    p.props.line_height = Some(LineHeight::Normal);
    p.def_props.size = Some(20.0);
    p.end_props.size = Some(20.0);
    p.runs = vec![
        Run::with("small", RunProps { size: Some(14.0), ..Default::default() }),
        Run::with(" LARGE", RunProps { size: Some(48.0), ..Default::default() }),
        Run { kind: RunKind::Break, ..Run::new("") },
        Run { kind: RunKind::Break, ..Run::new("") },
        Run::with("العربية 漢", RunProps { size: Some(20.0), ..Default::default() }),
    ];
    Shape { text: Some(body), text_box: true, ..Default::default() }
}

fn lay(shape: &Shape, height: f64) -> TextLayout {
    let p = Presentation::default();
    let ctx = Ctx::for_slide(&p, &p.slides[0]).unwrap();
    layout(
        &ctx,
        shape,
        shape.text.as_ref().unwrap(),
        &Opts { rect: Rect::new(0.0, 0.0, 900.0, height), fields: &NoFields, prompt_color: None, no_shrink: false },
    )
}

fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6, "{a} != {b}");
}

#[test]
fn mixed_later_run_and_empty_break_use_each_lines_natural_metrics() {
    let mut sh = shape();
    let natural = lay(&sh, 1000.0);
    assert_eq!(natural.lines.len(), 3);
    assert!(natural.lines[0].bottom - natural.lines[0].top > natural.lines[1].bottom - natural.lines[1].top);
    for minimum in [0.0, 20.0, 43.0, 90.0] {
        sh.text.as_mut().unwrap().paragraphs[0].props.line_height = Some(LineHeight::Minimum { points: minimum });
        let result = lay(&sh, 1000.0);
        let mut y = 0.0;
        for (line, base) in result.lines.iter().zip(&natural.lines) {
            let natural_pitch = base.bottom - base.top;
            let pitch = natural_pitch.max(minimum);
            near(line.top, y);
            near(line.bottom, y + pitch);
            near(line.baseline - line.top, (base.baseline - base.top) * pitch / natural_pitch);
            y += pitch;
        }
        near(result.content_height, y);
    }
}

#[test]
fn saved_and_recomputed_shrink_cannot_shrink_the_absolute_minimum() {
    for autofit in [AutoFit::Shrink { font_scale: Some(0.35), line_reduction: 0.2 }, AutoFit::Shrink { font_scale: Some(1.0), line_reduction: 0.0 }] {
        let mut sh = shape();
        let body = sh.text.as_mut().unwrap();
        body.body.autofit = Some(autofit);
        body.paragraphs[0].props.line_height = Some(LineHeight::Minimum { points: 65.0 });
        let result = lay(&sh, 70.0);
        assert!(result.overflow, "three absolute minimum pitches cannot fit in 70 points");
        assert!(result.font_scale < 1.0);
        assert!(result.line_reduction > 0.0);
        near(result.content_height, 195.0);
        for line in &result.lines {
            near(line.bottom - line.top, 65.0);
        }
    }
}

#[test]
fn anchors_carets_hits_selection_and_backgrounds_share_the_minimum_line_boxes() {
    for (anchor, top) in [(Anchor::Top, 0.0), (Anchor::Middle, 50.0), (Anchor::Bottom, 100.0)] {
        let mut sh = shape();
        let body = sh.text.as_mut().unwrap();
        body.body.anchor = Some(anchor);
        body.paragraphs[0].props.line_height = Some(LineHeight::Minimum { points: 80.0 });
        body.paragraphs[0].props.background =
            Some(deckcraft_model::Fill::solid(deckcraft_model::ColorRef::rgb(deckcraft_model::Rgba::rgb(40, 90, 150))));
        let result = lay(&sh, 340.0);
        near(result.content_height, 240.0);
        near(result.lines[0].top, top);
        for line in &result.lines {
            let pos = Pos { para: 0, ch: line.start };
            let (x, caret_top, caret_bottom) = result.caret(pos).unwrap();
            near(caret_top, line.top);
            near(caret_bottom, line.bottom);
            assert_eq!(result.hit(Point::new(x, (line.top + line.bottom) / 2.0)).para, 0);
        }
        let selections = result.selection_rects(Pos { para: 0, ch: 0 }, Pos { para: 0, ch: usize::MAX });
        assert!(!selections.is_empty());
        for rect in selections {
            assert!(result.lines.iter().any(|line| (line.top - rect.y0).abs() < 1e-6 && (line.bottom - rect.y1).abs() < 1e-6));
        }
        assert_eq!(result.backgrounds.len(), 1);
        near(result.backgrounds[0].rect.y0, top);
        near(result.backgrounds[0].rect.height(), 240.0);
    }
}

#[test]
fn dual_records_are_finite_and_keep_the_legacy_layout_fallback() {
    let mut sh = shape();
    let props = &mut sh.text.as_mut().unwrap().paragraphs[0].props;
    props.line_height = None;
    props.line_spacing = Some(Spacing::Pts(31.0));
    let legacy = lay(&sh, 1000.0);
    for policy in [LineHeight::Normal, LineHeight::Minimum { points: 999_999.0 }, LineHeight::Minimum { points: f64::NAN }] {
        sh.text.as_mut().unwrap().paragraphs[0].props.line_height = Some(policy);
        let result = lay(&sh, 1000.0);
        near(result.content_height, legacy.content_height);
        for (a, b) in result.lines.iter().zip(&legacy.lines) {
            near(a.top, b.top);
            near(a.bottom, b.bottom);
            near(a.baseline, b.baseline);
        }
    }
}
