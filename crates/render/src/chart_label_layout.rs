//! Automatic chart-label bounds and DrawingML overflow. The source body stays intact;
//! truncation belongs to the presentation of a label, never to its stored text.
use deckcraft_geom::Rect;
use deckcraft_model::Shape;
use deckcraft_model::resolve::Ctx;
use deckcraft_model::text::{Anchor, Paragraph, Run, RunKind, RunProps, TextBody, TextOverflow};
use deckcraft_text::{LayoutMetrics, LineMetrics, NoFields, Opts, TextLayout};

fn metrics(typographic: bool) -> LayoutMetrics {
    LayoutMetrics { line: if typographic { LineMetrics::Typographic } else { LineMetrics::Slide }, ..Default::default() }
}

pub(super) fn measure(ctx: &Ctx, body: &TextBody, width: f64, typographic: bool) -> TextLayout {
    let mut body = body.clone();
    body.body.anchor = Some(Anchor::Top);
    let shape = Shape { text: Some(body.clone()), ..Default::default() };
    deckcraft_text::layout_with_metrics(
        ctx,
        &shape,
        &body,
        &Opts { rect: Rect::new(0., 0., width, 1e6), fields: &NoFields, prompt_color: None, no_shrink: true },
        metrics(typographic),
    )
}

fn run_len(run: &Run) -> usize {
    if matches!(run.kind, RunKind::Break) { 1 } else { run.text.chars().count() }
}

fn props_at(body: &TextBody, para: usize, mut at: usize) -> RunProps {
    let Some(p) = body.paragraphs.get(para) else { return RunProps::default() };
    for run in &p.runs {
        let len = run_len(run);
        if at < len {
            return run.props.clone();
        }
        at = at.saturating_sub(len);
    }
    let mut props = p.end_props.clone();
    props.inherit(&p.def_props);
    props
}

fn prefix(body: &TextBody, para: usize, mut end: usize, marker: Option<&RunProps>) -> TextBody {
    let mut result =
        TextBody { body: body.body.clone(), list_style: body.list_style.clone(), paragraphs: body.paragraphs.iter().take(para).cloned().collect() };
    if let Some(source) = body.paragraphs.get(para) {
        let mut p = Paragraph {
            level: source.level,
            props: source.props.clone(),
            def_props: source.def_props.clone(),
            end_props: source.end_props.clone(),
            runs: Vec::new(),
        };
        for run in &source.runs {
            if end == 0 {
                break;
            }
            let len = run_len(run);
            let byte = if end < len { run.text.char_indices().nth(end).map_or(run.text.len(), |(i, _)| i) } else { run.text.len() };
            p.runs.push(Run { text: run.text.get(..byte).unwrap_or_default().into(), props: run.props.clone(), kind: run.kind.clone() });
            end = end.saturating_sub(len);
        }
        if let Some(props) = marker {
            p.runs.push(Run { text: "…".into(), props: props.clone(), kind: RunKind::Text });
        }
        result.paragraphs.push(p);
    }
    result
}

/// Native automatic column labels cap their height at half the chart frame, including
/// explicit no-wrap labels. Keep complete lines; an ellipsis takes the first omitted
/// character's style. Its width can require removing graphemes from the last kept line.
pub(super) fn overflow(ctx: &Ctx, body: &mut TextBody, width: f64, height: f64, measured: TextLayout) -> (TextLayout, Option<TextBody>) {
    let policy = body.body.vertical_overflow.unwrap_or(TextOverflow::Ellipsis);
    if policy == TextOverflow::Overflow {
        return (measured, None);
    }
    let bottom = body.body.inset_b.unwrap_or(0.);
    // An oversized first line is retained. The native chart frame can clip its ink,
    // but the automatic label never becomes empty just because that line exceeds the cap.
    let first_hidden = measured.lines.iter().enumerate().position(|(i, line)| i > 0 && line.bottom + bottom > height);
    let Some(first_hidden) = first_hidden else { return (measured, None) };
    let Some(last) = first_hidden.checked_sub(1).and_then(|i| measured.lines.get(i)) else { return (measured, None) };
    let marker =
        if policy == TextOverflow::Ellipsis { measured.lines.get(first_hidden).map(|line| props_at(body, line.para, line.start)) } else { None };
    let clipped = prefix(body, last.para, last.end, marker.as_ref());
    let candidate = measure(ctx, &clipped, width, true);
    if marker.is_none() || candidate.lines.len() <= first_hidden {
        let base = marker.as_ref().map(|_| prefix(body, last.para, last.end, None));
        let mut candidate = candidate;
        if let Some(base) = &base {
            marker_metrics(&mut candidate, &measure(ctx, base, width, true));
        }
        *body = clipped;
        return (candidate, base);
    }
    // Search only legal grapheme boundaries from the shaped line. This never cuts a
    // surrogate, combining sequence or joined emoji, and requires logarithmic layouts.
    let cuts: Vec<_> = measured
        .boundaries
        .get(last.para)
        .into_iter()
        .flatten()
        .enumerate()
        .skip(last.start)
        .take(last.end.saturating_sub(last.start).saturating_add(1))
        .filter_map(|(i, &boundary)| boundary.then_some(i))
        .collect();
    let (mut lo, mut hi) = (0, cuts.len());
    let mut best = prefix(body, last.para, last.start, marker.as_ref());
    let mut best_layout = measure(ctx, &best, width, true);
    let mut best_cut = last.start;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let Some(&cut) = cuts.get(mid) else { break };
        let trial = prefix(body, last.para, cut, marker.as_ref());
        let layout = measure(ctx, &trial, width, true);
        if layout.lines.len() <= first_hidden {
            best = trial;
            best_layout = layout;
            best_cut = cut;
            lo = mid.saturating_add(1);
        } else {
            hi = mid;
        }
    }
    let base = prefix(body, last.para, best_cut, None);
    marker_metrics(&mut best_layout, &measure(ctx, &base, width, true));
    *body = best;
    (best_layout, Some(base))
}

/// Native truncation inserts the marker on the retained baseline; a larger hidden
/// run does not enlarge the line or move preceding text. Keep its horizontal shaping
/// and restore the unmarked vertical geometry, including decorations.
fn marker_metrics(marked: &mut TextLayout, base: &TextLayout) {
    if marked.lines.len() != base.lines.len() {
        return;
    }
    let delta: Vec<_> = marked.lines.iter().zip(&base.lines).map(|(a, b)| b.baseline - a.baseline).collect();
    for run in &mut marked.runs {
        let Some(&(start, count)) = marked.para_lines.get(run.para) else { continue };
        let Some(lines) = marked.lines.get(start..start.saturating_add(count)) else { continue };
        let local = lines.partition_point(|line| line.end <= run.chars.0).min(count.saturating_sub(1));
        let dy = delta.get(start.saturating_add(local)).copied().unwrap_or(0.);
        for (_, _, y) in &mut run.glyphs {
            *y += dy;
        }
    }
    for deco in &mut marked.decos {
        let Some(&(start, count)) = marked.para_lines.get(deco.para) else { continue };
        let Some(lines) = marked.lines.get(start..start.saturating_add(count)) else { continue };
        let local = lines.partition_point(|line| line.top <= deco.rect.center().y).saturating_sub(1);
        let dy = delta.get(start.saturating_add(local)).copied().unwrap_or(0.);
        deco.rect = deco.rect + kurbo::Vec2::new(0., dy);
    }
    for (line, original) in marked.lines.iter_mut().zip(&base.lines) {
        line.top = original.top;
        line.baseline = original.baseline;
        line.bottom = original.bottom;
    }
    marked.content_height = base.content_height;
}

pub(super) fn placed(ctx: &Ctx, body: &TextBody, rect: Rect, marker_base: Option<&TextBody>, typographic: bool) -> TextLayout {
    let shape = Shape { text: Some(body.clone()), ..Default::default() };
    let opts = Opts { rect, fields: &NoFields, prompt_color: None, no_shrink: true };
    let mut layout = deckcraft_text::layout_with_metrics(ctx, &shape, body, &opts, metrics(typographic));
    if let Some(base) = marker_base {
        let mut base = base.clone();
        base.body = body.body.clone();
        let shape = Shape { text: Some(base.clone()), ..Default::default() };
        marker_metrics(&mut layout, &deckcraft_text::layout_with_metrics(ctx, &shape, &base, &opts, metrics(typographic)));
    }
    layout
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_model::text::{Align, Bullet};
    use deckcraft_model::{LayoutType, Presentation, defaults};

    fn document() -> Presentation {
        let mut p = defaults::new_presentation(None);
        let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap();
        let slide = defaults::new_slide(&mut p, layout).unwrap();
        p.slides = vec![std::sync::Arc::new(slide)];
        p
    }

    fn rows(size: f64) -> TextBody {
        let mut body = TextBody::from_text(&(1..=12).map(|i| format!("Row{i:02}")).collect::<Vec<_>>().join("\n"));
        body.body.inset_l = Some(3.);
        body.body.inset_r = Some(3.);
        body.body.inset_t = Some(1.5);
        body.body.inset_b = Some(1.5);
        body.body.wrap = Some(true);
        body.body.anchor = Some(Anchor::Top);
        for p in &mut body.paragraphs {
            p.props.bullet = Some(Bullet::None);
            p.props.align = Some(Align::Center);
            for run in &mut p.runs {
                run.props.size = Some(size);
                run.props.font = Some("Inter".into());
            }
        }
        body
    }

    #[test]
    fn automatic_height_keeps_whole_lines_and_ellipsis() {
        let p = document();
        let ctx = Ctx::for_slide(&p, &p.slides[0]).unwrap();
        for (height, count) in [(90., 2), (120., 3), (150., 4), (200., 5), (270., 7), (360., 9)] {
            let mut body = rows(30.);
            let measured = measure(&ctx, &body, 192., true);
            let (result, _) = overflow(&ctx, &mut body, 192., height, measured);
            assert_eq!(result.lines.len(), count, "height {height}");
            assert!(body.text().ends_with('…'));
            assert!(result.content_height <= height);
        }
    }

    #[test]
    fn clip_and_explicit_overflow_remain_distinct() {
        let p = document();
        let ctx = Ctx::for_slide(&p, &p.slides[0]).unwrap();
        for (policy, expected) in [(TextOverflow::Clip, 5), (TextOverflow::Overflow, 12)] {
            let mut body = rows(30.);
            body.body.vertical_overflow = Some(policy);
            let measured = measure(&ctx, &body, 192., true);
            let (result, _) = overflow(&ctx, &mut body, 192., 200., measured);
            assert_eq!(result.lines.len(), expected);
            assert!(!body.text().contains('…'));
        }
    }

    #[test]
    fn the_font_metrics_determine_how_many_lines_fit() {
        let p = document();
        let ctx = Ctx::for_slide(&p, &p.slides[0]).unwrap();
        for (font, expected) in [("Inter", 4), ("Open Sans", 3)] {
            let mut body = rows(30.);
            for p in &mut body.paragraphs {
                p.runs[0].props.font = Some(font.into());
            }
            let measured = measure(&ctx, &body, 192., true);
            let (result, _) = overflow(&ctx, &mut body, 192., 150., measured);
            assert_eq!(result.lines.len(), expected, "{font}");
        }
    }

    #[test]
    fn ellipsis_uses_hidden_style_and_removes_only_graphemes_that_do_not_fit() {
        let p = document();
        let ctx = Ctx::for_slide(&p, &p.slides[0]).unwrap();
        let mut body = rows(48.);
        let measured = measure(&ctx, &body, 192., true);
        let (result, _) = overflow(&ctx, &mut body, 192., 200., measured);
        assert_eq!(result.lines.len(), 3);
        assert!(body.text().ends_with("Row0…"), "{}", body.text());
        let mut body = rows(30.);
        body.paragraphs[5].runs[0].props.size = Some(18.);
        body.paragraphs[5].runs[0].props.bold = Some(true);
        let measured = measure(&ctx, &body, 192., true);
        overflow(&ctx, &mut body, 192., 190., measured);
        let marker = body.paragraphs.last().unwrap().runs.last().unwrap();
        assert_eq!(marker.text, "…");
        assert_eq!(marker.props.size, Some(18.));
        assert_eq!(marker.props.bold, Some(true));
    }

    #[test]
    fn larger_marker_keeps_the_retained_baseline_and_first_line_always_survives() {
        let p = document();
        let ctx = Ctx::for_slide(&p, &p.slides[0]).unwrap();
        let mut body = rows(72.);
        body.paragraphs[0].runs[0].props.size = Some(12.);
        let measured = measure(&ctx, &body, 192., true);
        let baseline = measured.lines[0].baseline;
        let (result, marker_base) = overflow(&ctx, &mut body, 192., 20., measured);
        assert_eq!(body.text(), "Row01…");
        assert_eq!(result.lines.len(), 1);
        assert!((result.lines[0].baseline - baseline).abs() < 1e-8);
        assert!(result.content_height < 20.);
        let painted = placed(&ctx, &body, Rect::new(0., 0., 192., 20.), marker_base.as_ref(), true);
        let baselines: Vec<_> = painted.runs.iter().flat_map(|r| r.glyphs.iter().map(|(_, _, y)| *y)).collect();
        assert!(baselines.iter().all(|y| (y - baseline).abs() < 1e-8));
        assert!(painted.runs.iter().any(|r| r.size == 72.));

        let mut body = rows(48.);
        let measured = measure(&ctx, &body, 192., true);
        let (result, _) = overflow(&ctx, &mut body, 192., 20., measured);
        assert_eq!(result.lines.len(), 1);
        assert!(!body.is_empty(), "a line taller than the cap remains visible");
    }

    #[test]
    fn prefix_keeps_soft_break_offsets_and_never_copies_the_hidden_suffix() {
        let mut body = rows(20.);
        body.paragraphs.truncate(1);
        let props = body.paragraphs[0].runs[0].props.clone();
        body.paragraphs[0].runs = vec![
            Run { text: "A\u{301}".into(), props: props.clone(), kind: RunKind::Text },
            Run { text: String::new(), props: props.clone(), kind: RunKind::Break },
            Run { text: "尾".repeat(100_000), props: props.clone(), kind: RunKind::Text },
        ];
        let clipped = prefix(&body, 0, 4, Some(&props));
        assert_eq!(clipped.text(), "A\u{301}\u{b}尾…");
        assert_eq!(clipped.paragraphs[0].runs.len(), 4);
        assert_eq!(body.paragraphs[0].runs[2].text.chars().count(), 100_000, "source content is untouched");
    }
}
