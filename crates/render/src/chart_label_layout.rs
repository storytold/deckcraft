//! Automatic chart-label bounds and DrawingML overflow. The source body stays intact;
//! truncation belongs to the presentation of a label, never to its stored text.
use deckcraft_geom::Rect;
use deckcraft_model::Shape;
use deckcraft_model::chart::ChartLayout;
use deckcraft_model::resolve::Ctx;
use deckcraft_model::text::{Align, Anchor, Bullet, Paragraph, Run, RunKind, RunProps, TextBody, TextOverflow};
use deckcraft_text::{AdvanceMetrics, LayoutMetrics, LineMetrics, NoFields, Opts, TextLayout};

/// The text box before rotation. Manual dimensions are fixed; automatic dimensions
/// are a wrapping/overflow budget and shrink to the retained text.
#[derive(Clone, Copy, Debug)]
pub(super) struct Bounds {
    pub width: f64,
    pub height: f64,
    pub fixed: bool,
}

pub(super) fn bounds(frame: (f64, f64), angle: f64, manual: Option<&ChartLayout>) -> Option<Bounds> {
    if !frame.0.is_finite() || !frame.1.is_finite() || frame.0 <= 0. || frame.1 <= 0. || frame.0 > 1e6 || frame.1 > 1e6 {
        return None;
    }
    // Raw angles outside this range do not have periodic native sizing behavior.
    // The two equivalent vertical quarter turns also have independent controls.
    let half_turn = angle.abs() == 180.;
    if !angle.is_finite() || (angle.abs() > 90. && angle.abs() != 270. && !half_turn) || (half_turn && manual.is_some()) {
        return None;
    }
    if let Some(l) = manual {
        if l.w_edge
            || l.h_edge
            || [l.x, l.y, l.w, l.h].into_iter().flatten().any(|v| !v.is_finite() || v.abs() > 1.)
            || l.w.is_some_and(|v| v < 0.)
            || l.h.is_some_and(|v| v < 0.)
            || ((l.x_edge || l.y_edge) && (l.x.is_none() || l.y.is_none()))
            || (l.x_edge && l.x.is_some_and(|v| v < 0.))
            || (l.y_edge && l.y.is_some_and(|v| v < 0.))
        {
            return None;
        }
        if l.w.is_some() || l.h.is_some() {
            // Specifying either dimension switches off automatic sizing. An absent
            // partner is zero, while a small positive dimension must remain visible.
            return Some(Bounds { width: l.w.unwrap_or(0.) * frame.0, height: l.h.unwrap_or(0.) * frame.1, fixed: true });
        }
    }
    let (a, b) = (frame.0 / 5., frame.1 / 2.);
    let (s, c) = angle.to_radians().sin_cos();
    let (s, c) = (s.abs(), c.abs());
    let (width, height) = if s < 1e-12 {
        (a, b)
    } else if c < 1e-12 {
        (b, a)
    } else if a.min(b) <= 2. * s * c * a.max(b) + 1e-12 * (a + b) {
        // The area optimum touches only the shorter pair of bounding edges.
        if a >= b { (b / (2. * s), b / (2. * c)) } else { (a / (2. * c), a / (2. * s)) }
    } else {
        // Both constraints are active: c*w+s*h=a and s*w+c*h=b.
        let d = c * c - s * s;
        ((a * c - b * s) / d, (b * c - a * s) / d)
    };
    (width.is_finite() && height.is_finite() && width > 0. && height > 0.).then_some(Bounds { width, height, fixed: false })
}

pub(super) fn rotated_extent(width: f64, height: f64, angle: f64) -> (f64, f64) {
    let (s, c) = angle.to_radians().sin_cos();
    (width * c.abs() + height * s.abs(), width * s.abs() + height * c.abs())
}

/// Place the rotated bounds, keeping the text box in its unrotated coordinates.
#[allow(clippy::too_many_arguments)]
pub(super) fn position(
    size: (f64, f64),
    angle: f64,
    mark: Rect,
    position: Option<&str>,
    negative: bool,
    frame: (f64, f64),
    manual: Option<&ChartLayout>,
    fixed: bool,
    clamp_to_frame: bool,
) -> Rect {
    let (w, h) = size;
    let (outer_w, outer_h) = rotated_extent(w, h, angle);
    let mut center = mark.center();
    match position {
        Some("ctr") => {
            // Centered labels stay inside the bar's baseline. Automatic labels
            // constrain their capped rotated bounds; manual dimensions describe
            // the box before rotation. Authored offsets are applied afterwards.
            let reserved_h = if fixed { h } else { outer_h.min(frame.1 / 2.) };
            center.y = if negative { center.y.max(mark.y0 + 3. + reserved_h / 2.) } else { center.y.min(mark.y1 - 3. - reserved_h / 2.) };
        }
        Some("inBase" | "inEnd") => {
            let end = position == Some("inEnd");
            center.y = if end != negative { mark.y0 + 3. + outer_h / 2. } else { mark.y1 - 3. - outer_h / 2. };
        }
        _ => center.y = if negative { mark.y1 + 3. + outer_h / 2. } else { mark.y0 - 3. - outer_h / 2. },
    }
    if let Some(l) = manual
        && let Some(x) = l.x
    {
        center.x = if l.x_edge { x * frame.0 + w / 2. } else { center.x + x * frame.0 };
        // Native factor positioning is enabled by the x coordinate. Missing y
        // means zero offset; a lone factor y retains the automatic position.
        if let Some(y) = l.y {
            center.y = if l.y_edge { y * frame.1 + h / 2. } else { center.y + y * frame.1 };
        }
    }
    if clamp_to_frame {
        let (clip_w, clip_h) = if fixed { (w, h) } else { (outer_w, outer_h) };
        center.x = (center.x - clip_w / 2.).clamp(0., (frame.0 - clip_w).max(0.)) + clip_w / 2.;
        center.y = (center.y - clip_h / 2.).clamp(0., (frame.1 - clip_h).max(0.)) + clip_h / 2.;
    }
    Rect::new(center.x - w / 2., center.y - h / 2., center.x + w / 2., center.y + h / 2.)
}

fn metrics(typographic: bool) -> LayoutMetrics {
    // Rich Column labels use fine nominal advances for both wrapping and glyph
    // placement, with rounded line-fit comparisons. This numeric policy is
    // independent of line height and does not mark the body as a diagram.
    LayoutMetrics {
        line: if typographic { LineMetrics::Typographic } else { LineMetrics::Slide },
        advance: if typographic { AdvanceMetrics::Chart } else { AdvanceMetrics::DrawingMl },
    }
}

/// Rich Column labels use the resolved top margin on both vertical sides, for
/// fitting and anchoring alike. Apply this only to the renderer's working body;
/// the authored bottom margin remains available for editing and serialization.
pub(super) fn column_vertical_insets(body: &mut TextBody) {
    body.body.inset_b = body.body.inset_t;
}

/// Any authored margin different from the chart defaults selects the custom
/// layout path, even a one-EMU difference or a bottom margin ignored in sizing.
/// Inspect this before normalizing the working body's vertical margins.
pub(super) fn custom_column_insets(body: &TextBody) -> bool {
    body.body.inset_l.unwrap_or(3.) != 3.
        || body.body.inset_r.unwrap_or(3.) != 3.
        || body.body.inset_t.unwrap_or(1.5) != 1.5
        || body.body.inset_b.unwrap_or(1.5) != 1.5
}

/// Minimal-width measurement can allocate one line per grapheme. Bound that
/// extra work, including empty paragraphs/runs, before using the ordinary path
/// for a larger body. Counting UTF-8 bytes conservatively bounds shaped cells.
pub(super) fn bounded_minimal_measurement(body: &TextBody) -> bool {
    let mut remaining = 16_384usize;
    for para in &body.paragraphs {
        let Some(next) = remaining.checked_sub(1) else { return false };
        remaining = next;
        for run in &para.runs {
            let Some(next) = remaining.checked_sub(run.text.len().max(1)) else { return false };
            remaining = next;
        }
    }
    true
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
pub(super) struct Marker {
    base: TextBody,
    paint: RunProps,
}

pub(super) fn overflow(ctx: &Ctx, body: &mut TextBody, width: f64, height: f64, measured: TextLayout) -> (TextLayout, Option<Marker>) {
    let policy = body.body.vert_overflow.unwrap_or(TextOverflow::Ellipsis);
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
        let marker = marker.as_ref().map(|paint| Marker { base: prefix(body, last.para, last.end, None), paint: paint.clone() });
        let mut candidate = candidate;
        if let Some(marker) = &marker {
            marker_metrics(&mut candidate, &measure(ctx, &marker.base, width, true));
        }
        *body = clipped;
        return (candidate, marker);
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
    // Width reservation keeps the initially hidden line's style. If fitting that
    // reservation removes more text, the painted marker takes the first removed
    // character's style without changing the reservation or fitting again.
    let paint = if best_cut < last.end { props_at(body, last.para, best_cut) } else { marker.unwrap_or_default() };
    marker_metrics(&mut best_layout, &measure(ctx, &base, width, true));
    *body = best;
    (best_layout, Some(Marker { base, paint }))
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

pub(super) fn placed(ctx: &Ctx, body: &TextBody, rect: Rect, marker: Option<&Marker>, typographic: bool) -> TextLayout {
    let shape = Shape { text: Some(body.clone()), ..Default::default() };
    let opts = Opts { rect, fields: &NoFields, prompt_color: None, no_shrink: true };
    let reserved = deckcraft_text::layout_with_metrics(ctx, &shape, body, &opts, metrics(typographic));
    let Some(marker) = marker else { return reserved };
    let mut base = marker.base.clone();
    base.body = body.body.clone();
    let shape = Shape { text: Some(base.clone()), ..Default::default() };
    let mut layout = deckcraft_text::layout_with_metrics(ctx, &shape, &base, &opts, metrics(typographic));
    if layout.lines.len() != reserved.lines.len() {
        return reserved;
    }
    let (Some(line), Some(full)) = (layout.lines.last().cloned(), reserved.lines.last()) else { return reserved };
    let width = line.extent.1 - line.extent.0;
    let left = if line.rtl { full.extent.1 - width } else { full.extent.0 };
    let dx = left - line.extent.0;
    shift_last_line(&mut layout, dx);
    // A marker is a drawing attachment at the paragraph's trailing side. It does
    // not acquire the adjacent Latin run's direction in an RTL paragraph.
    let mut mark = marker_layout(ctx, &marker.paint, typographic);
    let mx = if line.rtl { left - mark.content_width } else { left + width };
    let my = line.baseline - mark.lines.first().map_or(0., |l| l.baseline);
    for run in &mut mark.runs {
        run.para = line.para;
        run.chars = (line.end, line.end.saturating_add(1));
        for cell in &mut run.cells {
            cell.0 = line.end;
        }
        for (_, x, y) in &mut run.glyphs {
            *x += mx;
            *y += my;
        }
    }
    for deco in &mut mark.decos {
        deco.para = line.para;
        deco.rect = deco.rect + kurbo::Vec2::new(mx, my);
    }
    layout.runs.extend(mark.runs);
    layout.decos.extend(mark.decos);
    layout.content_width = reserved.content_width;
    layout
}

fn marker_layout(ctx: &Ctx, paint: &RunProps, typographic: bool) -> TextLayout {
    let mut body = TextBody::from_text("…");
    body.body.inset_l = Some(0.);
    body.body.inset_r = Some(0.);
    body.body.inset_t = Some(0.);
    body.body.inset_b = Some(0.);
    body.body.wrap = Some(false);
    for p in &mut body.paragraphs {
        p.props.align = Some(Align::Left);
        p.props.rtl = Some(false);
        p.props.bullet = Some(Bullet::None);
        p.props.margin_left = Some(0.);
        p.props.margin_right = Some(0.);
        p.props.indent = Some(0.);
        p.def_props = paint.clone();
        for run in &mut p.runs {
            run.props = paint.clone();
        }
    }
    measure(ctx, &body, 1e6, typographic)
}

fn shift_last_line(layout: &mut TextLayout, dx: f64) {
    let Some(line) = layout.lines.last() else { return };
    for run in &mut layout.runs {
        if run.para == line.para {
            for (i, (_, x, _)) in run.glyphs.iter_mut().enumerate() {
                if run.cells.get(i).is_some_and(|&(at, _)| at >= line.start && at < line.end) {
                    *x += dx;
                }
            }
        }
    }
    for deco in &mut layout.decos {
        if deco.para == line.para && deco.rect.center().y >= line.top {
            deco.rect = deco.rect + kurbo::Vec2::new(dx, 0.);
        }
    }
    if let Some(line) = layout.lines.last_mut() {
        line.extent.0 += dx;
        line.extent.1 += dx;
        line.start_offset += dx;
        for x in &mut line.caret_x {
            *x += dx;
        }
        for (a, b) in &mut line.edges {
            *a += dx;
            *b += dx;
        }
    }
}

/// Automatic markers affect line alignment but not the background's bounds. A
/// placed marked layout retains only the prefix in its line geometry, so mixed
/// alignment and RTL use the union of the aligned prefix extents.
pub(super) fn background_extent(layout: &TextLayout) -> Option<(f64, f64)> {
    layout.lines.iter().filter(|l| l.end > l.start).map(|l| l.extent).reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
}

/// Fixed chart labels take their horizontal alignment from the first paragraph.
/// The chart's default block centering includes the retained text, but excludes an
/// automatic marker. Defer it until the marker reservation has positioned the lines;
/// centering the reserved and unmarked bodies independently moves different lines.
/// This mutates only the renderer's working copy, preserving authored properties.
pub(super) fn manual_alignment(body: &mut TextBody) -> bool {
    if body.body.columns.is_some_and(|n| n > 1) {
        return false;
    }
    let Some(align @ (Align::Left | Align::Center | Align::Right)) = body.paragraphs.first().and_then(|p| p.props.align) else { return false };
    let center = body.body.anchor_ctr.unwrap_or(true) || align == Align::Center;
    body.body.anchor_ctr = Some(false);
    for p in &mut body.paragraphs {
        p.props.align = Some(align);
    }
    center
}

pub(super) fn centering_offset(layout: &TextLayout, body: &TextBody, rect: Rect) -> f64 {
    let Some((left, right)) = background_extent(layout) else { return 0. };
    let inner_center = (rect.x0 + body.body.inset_l.unwrap_or(0.) + rect.x1 - body.body.inset_r.unwrap_or(0.)) / 2.;
    let dx = inner_center - (left + right) / 2.;
    if dx.is_finite() { dx } else { 0. }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_model::text::{Align, Bullet};
    use deckcraft_model::{LayoutType, Presentation, defaults};

    #[test]
    fn rotated_budget_matches_independent_native_height_controls() {
        let cases = [
            ((800., 360.), 7., [11, 7]),
            ((800., 360.), 22., [10, 6]),
            ((800., 360.), 38., [8, 5]),
            ((800., 360.), 53., [6, 4]),
            ((800., 360.), 77., [8, 5]),
            ((420., 600.), 7., [20, 13]),
            ((420., 600.), 22., [7, 5]),
            ((420., 600.), 38., [4, 2]),
            ((420., 600.), 53., [3, 2]),
            ((420., 600.), 77., [2, 1]),
            ((1100., 260.), 7., [7, 4]),
            ((1100., 260.), 22., [4, 3]),
            ((1100., 260.), 38., [5, 3]),
            ((1100., 260.), 53., [7, 4]),
            ((1100., 260.), 77., [13, 9]),
        ];
        let p = document();
        let ctx = Ctx::for_slide(&p, &p.slides[0]).unwrap();
        for (frame, angle, counts) in cases {
            let b = bounds(frame, angle, None).unwrap();
            for (size, expected) in [12., 18.].into_iter().zip(counts) {
                let mut body = rows(size);
                let row = body.paragraphs[0].clone();
                body.paragraphs.resize(60, row);
                body.body.wrap = Some(false);
                let measured = measure(&ctx, &body, 1e6, true);
                let (result, _) = overflow(&ctx, &mut body, 1e6, b.height, measured);
                assert_eq!(result.lines.len(), expected, "{frame:?} at {angle}, {size}pt");
            }
        }
    }

    #[test]
    fn rotated_budget_is_bounded_at_axes_and_diagonals() {
        for frame in [(800., 320.), (800., 600.), (600., 320.)] {
            for angle in [0., 1e-10, 30., 44.9999999999, 45., 45.0000000001, 89.9999999999, 90., -90., 180., -180., 270.] {
                let b = bounds(frame, angle, None).unwrap();
                let outer = rotated_extent(b.width, b.height, angle);
                assert!(b.width > 0. && b.height > 0. && b.width.is_finite() && b.height.is_finite());
                assert!(outer.0 <= frame.0 / 5. + 1e-6 && outer.1 <= frame.1 / 2. + 1e-6, "{frame:?}, {angle}: {outer:?}");
            }
        }
        for frame in [(0., 400.), (-1., 400.), (f64::NAN, 400.), (1e9, 400.)] {
            assert!(bounds(frame, 30., None).is_none());
        }
        for angle in [91., 179., 181., 360., f64::NAN, f64::INFINITY] {
            assert!(bounds((800., 400.), angle, None).is_none());
        }
    }

    #[test]
    fn manual_dimensions_do_not_rotate_or_invent_missing_extents() {
        for angle in [0., 35., 90.] {
            let l = ChartLayout { w: Some(0.15), h: Some(0.18), ..Default::default() };
            let b = bounds((800., 360.), angle, Some(&l)).unwrap();
            assert!(b.fixed);
            assert!((b.width - 120.).abs() < 1e-6 && (b.height - 64.8).abs() < 1e-6);
            let missing = ChartLayout { h: None, ..l };
            assert_eq!(bounds((800., 360.), angle, Some(&missing)).unwrap().height, 0.);
        }
        let tiny = ChartLayout { w: Some(0.001), h: Some(0.001), ..Default::default() };
        let b = bounds((800., 360.), 0., Some(&tiny)).unwrap();
        assert_eq!((b.width, b.height), (0.8, 0.36));
        let invalid = ChartLayout { w: Some(f64::NAN), ..Default::default() };
        assert!(bounds((800., 360.), 0., Some(&invalid)).is_none());
        assert!(bounds((800., 360.), 180., Some(&tiny)).is_none());
    }

    #[test]
    fn positions_use_rotated_edges_and_fixed_three_point_gap() {
        let mark = Rect::new(200., 150., 400., 350.);
        for angle in [0., 37., 90.] {
            let size = (90., 30.);
            let outer = rotated_extent(size.0, size.1, angle);
            for negative in [false, true] {
                let inside = position(size, angle, mark, Some("inEnd"), negative, (800., 500.), None, false, true);
                let outside = position(size, angle, mark, Some("outEnd"), negative, (800., 500.), None, false, true);
                if negative {
                    assert!((mark.y1 - inside.center().y - outer.1 / 2. - 3.).abs() < 1e-6);
                    assert!((outside.center().y - outer.1 / 2. - mark.y1 - 3.).abs() < 1e-6);
                } else {
                    assert!((inside.center().y - outer.1 / 2. - mark.y0 - 3.).abs() < 1e-6);
                    assert!((mark.y0 - outside.center().y - outer.1 / 2. - 3.).abs() < 1e-6);
                }
            }
        }
    }

    #[test]
    fn centered_labels_respect_the_baseline_before_authored_offsets_and_frame_clipping() {
        let positive = Rect::new(326., 294., 538., 320.);
        let negative = Rect::new(326., 60., 538., 86.);
        for (mark, sign, expected) in [(positive, false, 224.7625), (negative, true, 155.2375)] {
            let r = position((118.325, 184.475), 0., mark, Some("ctr"), sign, (960., 400.), None, false, true);
            assert!((r.center().y - expected).abs() < 1e-6);
        }
        // No-wrap overflow keeps the full body centered, but uses the automatic
        // height cap to constrain that center. It may still extend past the frame.
        for (mark, sign, expected) in [(positive, false, 217.), (negative, true, 163.)] {
            let r = position((66.095, 264.3), 0., mark, Some("ctr"), sign, (960., 400.), None, false, false);
            assert!((r.center().y - expected).abs() < 1e-6);
        }
        let manual = ChartLayout { x: Some(0.), y: Some(0.1), w: Some(0.15), h: Some(0.18), ..Default::default() };
        let r = position((144., 72.), 0., positive, Some("ctr"), false, (960., 400.), Some(&manual), true, true);
        assert_eq!((r.y0, r.y1), (285., 357.));
        let r = position((144., 320.), 0., positive, Some("ctr"), false, (960., 400.), None, true, true);
        assert_eq!((r.y0, r.y1), (0., 320.));
        let r = position((144., 320.), 0., negative, Some("ctr"), true, (960., 400.), None, true, true);
        assert_eq!((r.y0, r.y1), (63., 383.));
        // A wide rotated manual box still positions its unrotated 72pt height.
        let r = position((480., 72.), 37., positive, Some("ctr"), false, (960., 400.), None, true, true);
        assert_eq!(r.center().y, 281.);
        assert!(r.center().y + rotated_extent(480., 72., 37.).1 / 2. > 450.);
    }

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
    fn column_vertical_insets_control_the_retained_lines_and_height() {
        let p = document();
        let ctx = Ctx::for_slide(&p, &p.slides[0]).unwrap();
        for (font, size, top, angle, count, height) in [
            ("Inter", 18., 7., 0., 6, 144.65),
            ("Inter", 18., 2., 0., 6, 134.65),
            ("Inter", 18., 1.5, 0., 6, 133.65),
            ("Lato", 22., 7., 37., 3, 93.2),
        ] {
            for bottom in [1., 2., 20.] {
                let mut body = rows(size);
                body.body.inset_l = Some(5.);
                body.body.inset_r = Some(1.);
                body.body.inset_t = Some(top);
                body.body.inset_b = Some(bottom);
                for para in &mut body.paragraphs {
                    para.runs[0].props.font = Some(font.into());
                }
                column_vertical_insets(&mut body);
                let budget = bounds((700., 300.), angle, None).unwrap();
                let measured = measure(&ctx, &body, budget.width, true);
                let (layout, marker) = overflow(&ctx, &mut body, budget.width, budget.height, measured);
                assert_eq!(layout.lines.len(), count, "{font}, angle={angle}, top={top}, bottom={bottom}");
                assert!(marker.is_some());
                assert!(
                    (layout.content_height + 2. * top - height).abs() < 0.05,
                    "{font}, angle={angle}, top={top}, bottom={bottom}: content height {}",
                    layout.content_height
                );
            }
        }
    }

    #[test]
    fn custom_column_insets_preserve_the_authored_default_boundary() {
        let original = rows(18.);
        assert!(!custom_column_insets(&original));
        assert!(!custom_column_insets(&TextBody::default()));
        for side in 0..4 {
            let mut body = original.clone();
            let inset = match side {
                0 => &mut body.body.inset_l,
                1 => &mut body.body.inset_r,
                2 => &mut body.body.inset_t,
                _ => &mut body.body.inset_b,
            };
            *inset = Some(inset.unwrap() + 1. / 12700.);
            assert!(custom_column_insets(&body), "one EMU on side {side}");
            column_vertical_insets(&mut body);
            if side == 3 {
                assert!(!custom_column_insets(&body), "capture the authored flag before normalizing bottom");
            }
        }
    }

    #[test]
    fn minimal_half_turn_measurement_and_final_width_painting_have_distinct_breaks() {
        let p = document();
        let ctx = Ctx::for_slide(&p, &p.slides[0]).unwrap();
        for (font, size, text, measured_lines, painted_lines, width, height) in [
            ("Inter", 24., "IIIIW", 5, 3, 23.65, 145.175),
            ("Open Sans", 18., "IIIIW", 5, 3, 16.62, 122.575),
            ("Inter", 24., "a\u{301}IIW", 4, 3, 23.65, 116.14),
            ("Inter", 24., "I I W", 3, 2, 23.65, 87.105),
            ("Open Sans", 18., "Thin Wide", 8, 6, 16.62, 196.12),
            ("Lato", 20., "01a1I", 5, 5, 11.6, 120.),
        ] {
            let mut body = rows(size);
            body.paragraphs.truncate(1);
            body.paragraphs[0].runs[0].text = text.into();
            body.paragraphs[0].runs[0].props.font = Some(font.into());
            body.body.anchor = Some(Anchor::Middle);
            let measured = measure(&ctx, &body, 0., true);
            assert_eq!(measured.lines.len(), measured_lines, "{font} {text}: measurement");
            assert!((measured.content_width - width).abs() < 0.01, "{font} {text}: {}", measured.content_width);
            assert!((measured.content_height - height).abs() < 0.1, "{font} {text}: {}", measured.content_height);
            let rect = Rect::new(0., 0., measured.content_width + 6., measured.content_height + 3.);
            let painted = placed(&ctx, &body, rect, None, true);
            assert_eq!(painted.lines.len(), painted_lines, "{font} {text}: painting at grown width");
        }
    }

    #[test]
    fn minimal_width_work_is_bounded_for_text_and_empty_structure() {
        let mut body = rows(18.);
        body.paragraphs.truncate(1);
        body.paragraphs[0].runs[0].text = "I".repeat(16_383);
        assert!(bounded_minimal_measurement(&body));
        body.paragraphs[0].runs[0].text.push('I');
        assert!(!bounded_minimal_measurement(&body));
        body.paragraphs[0].runs[0].text = "é".repeat(8192);
        assert!(!bounded_minimal_measurement(&body));
        body.paragraphs = vec![Paragraph::default(); 16_384];
        assert!(bounded_minimal_measurement(&body));
        body.paragraphs.push(Paragraph::default());
        assert!(!bounded_minimal_measurement(&body));
        body.paragraphs.truncate(1);
        body.paragraphs[0].runs = vec![Run::new(""); 16_384];
        assert!(!bounded_minimal_measurement(&body));
    }

    #[test]
    fn manual_alignment_centers_retained_text_without_its_attached_marker() {
        let p = document();
        let ctx = Ctx::for_slide(&p, &p.slides[0]).unwrap();
        let rect = Rect::new(300., 150., 420., 214.8);
        for (align, anchor_ctr, expected) in [
            (Align::Left, None, [330.2775, 330.2775]),
            (Align::Left, Some(false), [303., 303.]),
            (Align::Left, Some(true), [330.2775, 330.2775]),
            (Align::Center, None, [336.91, 327.305]),
            (Align::Center, Some(false), [336.91, 327.305]),
            (Align::Right, None, [341.715, 322.5]),
            (Align::Right, Some(false), [361.215, 342.]),
        ] {
            let mut body = rows(18.);
            body.paragraphs.truncate(4);
            body.paragraphs[0].props.align = Some(align);
            body.body.anchor_ctr = anchor_ctr;
            let center = manual_alignment(&mut body);
            let measured = measure(&ctx, &body, rect.width(), true);
            let (_, marker) = overflow(&ctx, &mut body, rect.width(), rect.height(), measured);
            let layout = placed(&ctx, &body, rect, marker.as_ref(), true);
            assert!(marker.is_some());
            assert_eq!(layout.lines.len(), 2);
            let dx = if center { centering_offset(&layout, &body, rect) } else { 0. };
            for (line, expected) in layout.lines.iter().zip(expected) {
                assert!((line.extent.0 + dx - expected).abs() < 0.005, "{align:?}, anchorCtr={anchor_ctr:?}: {line:?}");
            }
        }
    }

    #[test]
    fn manual_alignment_uses_the_first_paragraph_and_preserves_explicit_edge_anchoring() {
        let p = document();
        let ctx = Ctx::for_slide(&p, &p.slides[0]).unwrap();
        for (first, rest, expected) in [
            (Align::Center, Align::Left, [324.53, 349.805]),
            (Align::Center, Align::Right, [324.53, 349.805]),
            (Align::Left, Align::Center, [303., 303.]),
        ] {
            let mut body = rows(18.);
            body.paragraphs.truncate(4);
            for (i, para) in body.paragraphs.iter_mut().enumerate() {
                para.props.align = Some(if i == 0 { first } else { rest });
                para.runs[0].text = if i == 0 { "WWWW" } else { "I" }.into();
            }
            body.body.anchor_ctr = Some(false);
            let center = manual_alignment(&mut body);
            let measured = measure(&ctx, &body, 120., true);
            let (_, marker) = overflow(&ctx, &mut body, 120., 64.8, measured);
            let rect = Rect::new(300., 150., 420., 214.8);
            let layout = placed(&ctx, &body, rect, marker.as_ref(), true);
            let dx = if center { centering_offset(&layout, &body, rect) } else { 0. };
            assert_eq!(layout.lines.len(), 2);
            for (line, expected) in layout.lines.iter().zip(expected) {
                assert!((line.extent.0 + dx - expected).abs() < 0.005, "{first:?}/{rest:?}");
            }
        }
    }

    #[test]
    fn rich_label_break_thresholds_use_fine_nominal_advances() {
        let p = document();
        let ctx = Ctx::for_slide(&p, &p.slides[0]).unwrap();
        // Original native controls distinguish 1/200pt advances from both
        // continuous metrics and ordinary slide text's 1/8pt advances.
        for (font, size, text, width, end) in [
            ("Inter", 30., "Row10", 80., 3),
            ("Inter", 30., "Row10", 80.042, 3),
            ("Inter", 30., "Row10", 80.043, 4),
            ("Inter", 30., "Row10", 80.045, 4),
            ("Inter", 30., "Row1a", 80., 3),
            ("Inter", 30., "Row1a", 80.2, 4),
            ("Inter", 30., "Rowaa", 80., 3),
            ("Inter", 30., "1111111", 80., 6),
            ("Lato", 22., "IIIIIIIIX", 55.14, 7),
            ("Lato", 22., "IIIIIIIIX", 55.3, 8),
            ("Open Sans", 28., "Alpha beta gamma", 146.297, 5),
            ("Open Sans", 28., "Alpha beta gamma", 146.299, 10),
            ("Open Sans", 28., "IIIX", 29.459, 3),
            ("Open Sans", 28., "IIIX", 29.457, 2),
        ] {
            let mut body = rows(size);
            body.paragraphs.truncate(1);
            body.paragraphs[0].runs[0].text = text.into();
            body.paragraphs[0].runs[0].props.font = Some(font.into());
            let layout = measure(&ctx, &body, width, true);
            assert_eq!(layout.lines[0].end, end, "{font} {text} at {width}");
        }
    }

    #[test]
    fn rich_label_placed_glyphs_share_measurement_precision() {
        let p = document();
        let ctx = Ctx::for_slide(&p, &p.slides[0]).unwrap();
        for (font, size, fine, coarse) in [("Inter", 30., 8.055, 8.), ("Lato", 22., 6.16, 6.125)] {
            let mut body = rows(size);
            body.paragraphs.truncate(1);
            body.paragraphs[0].runs[0].text = "I".repeat(48);
            body.paragraphs[0].runs[0].props.font = Some(font.into());
            body.body.wrap = Some(false);
            for (rich_column, advance) in [(true, fine), (false, coarse)] {
                let layout = placed(&ctx, &body, Rect::new(0., 0., 500., 100.), None, rich_column);
                let glyphs: Vec<_> = layout.runs.iter().flat_map(|r| r.glyphs.iter()).collect();
                assert_eq!(glyphs.len(), 48);
                assert!((glyphs[47].1 - glyphs[0].1 - 47. * advance).abs() < 1e-6, "{font}, rich={rich_column}");
                assert!((layout.content_width - 48. * advance).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn fitted_rich_labels_retain_the_measured_lines() {
        let p = document();
        let ctx = Ctx::for_slide(&p, &p.slides[0]).unwrap();
        let mut body = rows(28.);
        body.paragraphs.truncate(1);
        body.paragraphs[0].runs[0].text = "Alpha beta gamma delta epsilon zeta".into();
        body.paragraphs[0].runs[0].props.font = Some("Open Sans".into());
        body.body.vert_overflow = Some(TextOverflow::Clip);
        let b = bounds((900., 280.), 12., None).unwrap();
        let measured = measure(&ctx, &body, b.width, true);
        let (measured, marker) = overflow(&ctx, &mut body, b.width, b.height, measured);
        let width = measured.content_width + 6.;
        let height = measured.content_height + 3.;
        let result = placed(&ctx, &body, Rect::new(0., 0., width, height), marker.as_ref(), true);
        assert_eq!(measured.lines.len(), 2);
        assert_eq!(result.lines.len(), measured.lines.len(), "width={width}, body={}", body.text());
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
            body.body.vert_overflow = Some(policy);
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
