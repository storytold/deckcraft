//! Charts: title, plot area with axes and gridlines, series marks, legend.

use std::collections::HashMap;

use deckcraft_color::{ColorTransform, Rgba, SchemeSlot};
use deckcraft_model::Shape;
use deckcraft_model::chart::{
    Chart, ChartAxisCrossing, ChartLayout, ChartLineRole, ChartTextRole, ChartTickPosition, ChartType, TrendlineType, axis_scale,
};
use deckcraft_model::resolve::Ctx;
use deckcraft_model::style::{ColorRef, Fill};
use deckcraft_model::text::{Align, Anchor, Bullet, RunProps, TextBody};
use kurbo::{Affine, BezPath, Point, Rect, Shape as _};
use vello_cpu::RenderContext;

use crate::{color, scale_of};

#[path = "chart_label_layout.rs"]
mod label_layout;

const SLOTS: [SchemeSlot; 6] =
    [SchemeSlot::Accent1, SchemeSlot::Accent2, SchemeSlot::Accent3, SchemeSlot::Accent4, SchemeSlot::Accent5, SchemeSlot::Accent6];

/// Colour of series/point `i`: accents, then darker and lighter rounds.
pub fn series_color(ctx: &Ctx, c: &Chart, i: usize) -> Rgba {
    let slot = SLOTS.get(i % 6).copied().unwrap_or(SchemeSlot::Accent1);
    let base = ctx.color(&ColorRef::scheme(slot), None);
    let mono = c.palette.starts_with("monochrome");
    let base = if mono { ctx.color(&ColorRef::scheme(SchemeSlot::Accent1), None) } else { base };
    let round = if mono { i } else { i / 6 };
    match round {
        0 => base,
        1 => deckcraft_color::apply(base, &[ColorTransform::Shade(60000)]),
        2 => deckcraft_color::apply(base, &[ColorTransform::Tint(60000)]),
        n => deckcraft_color::apply(base, &[ColorTransform::LumMod(100000 - (n as i32 % 5) * 12000)]),
    }
}

/// A series' one colour (lines, areas, markers): its solid fill, a pattern's foreground, else the
/// palette's.
pub(crate) fn series_fill(ctx: &Ctx, c: &Chart, i: usize, s: Option<&deckcraft_model::chart::Series>) -> Rgba {
    match s.and_then(|s| s.fill.as_ref()) {
        Some(Fill::Solid { color: cc }) => ctx.color(cc, None),
        Some(Fill::Pattern(p)) => ctx.color(&p.fg, None),
        _ => series_color(ctx, c, i),
    }
}

/// How a bar, or its legend key, is painted.
#[derive(Clone, Debug, PartialEq)]
enum Paint {
    Color(Rgba),
    Line(Rgba),
    Marker(Rgba),
    /// A preset pattern: name, foreground, background.
    Pattern(String, Rgba, Rgba),
    /// `a:noFill`: only the outline, if any, shows.
    Nothing,
}

/// The paint of fill `f` as a bar or legend key draws it; the palette's `fallback` when the file
/// gives none (or one a bar can't show).
fn paint_of(ctx: &Ctx, f: Option<&Fill>, fallback: Rgba) -> Paint {
    match f {
        Some(Fill::Solid { color: cc }) => Paint::Color(ctx.color(cc, None)),
        Some(Fill::Pattern(p)) => Paint::Pattern(p.preset.clone(), ctx.color(&p.fg, None), ctx.color(&p.bg, None)),
        Some(Fill::None) => Paint::Nothing,
        _ => Paint::Color(fallback),
    }
}

/// Fill `r` (in the space `m` maps to the output) with `p`.
fn fill_with(ctx: &mut RenderContext, p: &Paint, r: Rect, m: Affine) {
    match p {
        Paint::Color(c) => {
            ctx.set_transform(m);
            ctx.set_paint(color(*c, 1.0));
            ctx.fill_rect(&r);
        }
        Paint::Line(c) => {
            ctx.set_transform(m);
            ctx.set_paint(color(*c, 1.0));
            ctx.set_stroke(kurbo::Stroke::new(1.5));
            ctx.stroke_path(&polyline(&[Point::new(r.x0, r.center().y), Point::new(r.x1, r.center().y)]));
        }
        Paint::Marker(c) => {
            ctx.set_transform(m);
            ctx.set_paint(color(*c, 1.0));
            ctx.fill_path(&kurbo::Circle::new(r.center(), r.width().min(r.height()) * 0.5).to_path(0.1));
        }
        Paint::Pattern(name, fg, bg) => crate::pattern::fill(ctx, name, *fg, *bg, 1.0, &r.to_path(0.1), m),
        Paint::Nothing => {}
    }
}

/// A bar series' outline (`c:spPr/a:ln`) where the file gives it a solid colour: colour and width.
fn bar_outline(ctx: &Ctx, s: &deckcraft_model::chart::Series) -> Option<(Rgba, f64)> {
    let l = s.line.as_ref()?;
    match l.fill.as_ref()? {
        Fill::Solid { color: cc } => Some((ctx.color(cc, None), l.width.unwrap_or(0.75).clamp(0.0, 20.0))),
        _ => None,
    }
}

/// Paint of one bar: a `<c:dPt>` fill wins, and a single-series chart that varies colours by point uses one accent per point
/// (not waterfalls, which are never read from a `<c:barChart>`). Pattern fills draw as patterns and
/// `a:noFill` leaves the bar empty, as in PowerPoint.
fn bar_paint(ctx: &Ctx, c: &Chart, si: usize, pi: usize, s: &deckcraft_model::chart::Series, fills: Option<&HashMap<usize, &Fill>>) -> Paint {
    if let Some(f) = fills.and_then(|m| m.get(&pi)) {
        return paint_of(ctx, Some(*f), series_fill(ctx, c, si, Some(s)));
    }
    if c.vary_colors && c.series.len() == 1 && s.fill.is_none() && c.kind != ChartType::Waterfall {
        return Paint::Color(series_color(ctx, c, pi));
    }
    paint_of(ctx, s.fill.as_ref(), series_color(ctx, c, si))
}

fn polyline(pts: &[Point]) -> BezPath {
    let mut p = BezPath::new();
    for (k, q) in pts.iter().enumerate() {
        if k == 0 {
            p.move_to(*q);
        } else {
            p.line_to(*q);
        }
    }
    p
}

/// A smoothed line (`c:smooth`) through `pts`: a Catmull-Rom spline as cubic Béziers, the curve
/// PowerPoint draws, passing through every point.
pub(crate) fn smooth_path(pts: &[Point]) -> BezPath {
    let mut p = BezPath::new();
    let Some(first) = pts.first() else { return p };
    p.move_to(*first);
    for k in 0..pts.len() - 1 {
        let p0 = pts[k.saturating_sub(1)];
        let (p1, p2) = (pts[k], pts[k + 1]);
        let p3 = pts[(k + 2).min(pts.len() - 1)];
        let c1 = p1 + (p2 - p0) / 6.0;
        let c2 = p2 - (p3 - p1) / 6.0;
        p.curve_to(c1, c2, p2);
    }
    p
}

/// A line series' colour: its line's own (where PowerPoint keeps it), else its fill's, else the
/// palette's.
pub(crate) fn series_line(ctx: &Ctx, c: &Chart, i: usize, s: &deckcraft_model::chart::Series) -> Rgba {
    match s.line.as_ref().and_then(|l| l.fill.as_ref()) {
        Some(Fill::Solid { color: cc }) => ctx.color(cc, None),
        _ => series_fill(ctx, c, i, Some(s)),
    }
}

/// A category label on two lines, split at its middle space or dash ("03.11.2025 - 09.11.2025").
fn split_label(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mid = chars.len() / 2;
    let best = chars.iter().enumerate().filter(|(_, c)| c.is_whitespace()).map(|(i, _)| i).min_by_key(|i| i.abs_diff(mid));
    match best {
        Some(i) if i > 0 && i + 1 < chars.len() => {
            let (a, b) = chars.split_at(i);
            format!("{}\n{}", a.iter().collect::<String>().trim_end(), b.iter().collect::<String>().trim_start())
        }
        _ => name.to_string(),
    }
}

pub(crate) struct LabelStyle {
    pub(crate) size: f64,
    pub(crate) align: Align,
    pub(crate) anchor: Anchor,
    pub(crate) col: Rgba,
    pub(crate) bold: bool,
}

pub(crate) fn label(ctx: &mut RenderContext, rctx: &Ctx, m: Affine, text: &str, r: Rect, style: LabelStyle) {
    styled_label(ctx, rctx, m, text, r, style, &RunProps::default());
}

fn label_body(text: &str, style: LabelStyle, props: &RunProps) -> TextBody {
    let LabelStyle { size, align, anchor, col, bold } = style;
    let mut body = TextBody::from_text(text);
    body.body.inset_l = Some(0.0);
    body.body.inset_r = Some(0.0);
    body.body.inset_t = Some(0.0);
    body.body.inset_b = Some(0.0);
    body.body.anchor = Some(anchor);
    body.body.wrap = Some(false);
    for p in &mut body.paragraphs {
        p.props.align = Some(align);
        p.props.bullet = Some(Bullet::None);
        p.props.margin_left = Some(0.0);
        p.props.indent = Some(0.0);
        for run in &mut p.runs {
            run.props = props.clone();
            run.props.inherit(&RunProps { size: Some(size), bold: Some(bold), fill: Some(Fill::solid(ColorRef::rgb(col))), ..Default::default() });
        }
    }
    body
}

fn styled_label(ctx: &mut RenderContext, rctx: &Ctx, m: Affine, text: &str, r: Rect, style: LabelStyle, props: &RunProps) {
    let body = label_body(text, style, props);
    let tmp = Shape { text: Some(body.clone()), ..Default::default() };
    crate::draw_text(
        ctx,
        rctx,
        &tmp,
        &body,
        m,
        &deckcraft_text::Opts { rect: r, fields: &deckcraft_text::NoFields, prompt_color: None, no_shrink: false },
    );
}

/// Chart text keeps body rotation/vertical layout as well as run typography.
fn chart_label(ctx: &mut RenderContext, rctx: &Ctx, m: Affine, text: &str, r: Rect, style: LabelStyle, chart: (&Chart, ChartTextRole, bool)) {
    let (c, role, axis_tick) = chart;
    let line = if matches!(role, ChartTextRole::CategoryAxis | ChartTextRole::ValueAxis | ChartTextRole::SecondaryValueAxis) {
        deckcraft_text::LineMetrics::Typographic
    } else {
        deckcraft_text::LineMetrics::Slide
    };
    chart_label_with_metrics(ctx, rctx, m, text, r, style, (c, role, axis_tick, line, None, None));
}

fn chart_label_with_metrics(
    ctx: &mut RenderContext,
    rctx: &Ctx,
    m: Affine,
    text: &str,
    r: Rect,
    style: LabelStyle,
    chart: (&Chart, ChartTextRole, bool, deckcraft_text::LineMetrics, Option<Affine>, Option<(f64, f64)>),
) {
    let (c, role, axis_tick, line, view, capacity) = chart;
    let mut props = c.text_props(role);
    if axis_tick && role == ChartTextRole::ValueAxis {
        // Generated value ticks retain nominal advances independently of the
        // authored kerning threshold. Keep that property in the chart model.
        props.kern = Some(0.);
    }
    if axis_tick && props.fill.is_none() {
        // Legacy chart styles use mapped text color, except the light-text
        // styles, which use the theme's light slot directly. A sibling axis's
        // explicit fill is not an inherited chart-wide text color.
        let slot = if c.style.clamp(1, 48) >= 41 { SchemeSlot::Lt1 } else { SchemeSlot::Tx1 };
        props.fill = Some(Fill::solid(ColorRef::scheme(slot)));
    }
    let anchor = style.anchor;
    let mut body = label_body(text, style, &props);
    if let Some(explicit) = c.format.bodies.get(&role) {
        body.body = merged_chart_body(explicit, &body.body);
    }
    if axis_tick {
        // Tick text uses its axis placement and label distance, independently of
        // text-box margins. Preserve the authored margins in the model only.
        body.body.inset_l = Some(0.);
        body.body.inset_r = Some(0.);
        body.body.inset_t = Some(0.);
        body.body.inset_b = Some(0.);
        if matches!(line, deckcraft_text::LineMetrics::Chart | deckcraft_text::LineMetrics::ChartLow) {
            // Category ticks use the axis-selected side of their box;
            // an authored text-body anchor does not move the generated label.
            body.body.anchor = Some(anchor);
        }
    }
    for p in &mut body.paragraphs {
        p.props.line_spacing = c.line_spacing(role).cloned();
    }
    let tmp = Shape { text: Some(body.clone()), ..Default::default() };
    let mut layout = deckcraft_text::layout_with_metrics(
        rctx,
        &tmp,
        &body,
        &deckcraft_text::Opts { rect: r, fields: &deckcraft_text::NoFields, prompt_color: None, no_shrink: true },
        deckcraft_text::LayoutMetrics { line, ..Default::default() },
    );
    if let Some(capacity) = capacity {
        layout = category_tick_overflow(rctx, &mut body, r, layout, capacity);
    }
    if let Some(view) = view {
        crate::text_draw::draw_layout_with_origins(ctx, &layout, r, m, view);
    } else {
        crate::draw_layout(ctx, &layout, r, m);
    }
}

/// Generated upright category ticks fit complete paragraph pitches inside the
/// chart, including its 1.5pt label margin. A marker reserves width and kerning
/// during line alignment; the retained literal text still defines placement.
fn category_tick_overflow(
    rctx: &Ctx,
    body: &mut TextBody,
    rect: Rect,
    layout: deckcraft_text::TextLayout,
    (pitch, frame_height): (f64, f64),
) -> deckcraft_text::TextLayout {
    use deckcraft_model::text::TextOverflow;
    let policy = body.body.vert_overflow.unwrap_or(TextOverflow::Ellipsis);
    if policy == TextOverflow::Overflow || !pitch.is_finite() || pitch <= 0. || !frame_height.is_finite() {
        return layout;
    }
    // Retain an oversized first line; the chart's existing frame clip still
    // bounds its ink. This pass uses the already bounded text layout, with no
    // additional per-chart label queue or repeated fitting search.
    let first_hidden = layout.lines.iter().enumerate().position(|(i, _)| i > 0 && rect.y0 + (i + 1) as f64 * pitch + 1.5 > frame_height);
    let Some(last) = first_hidden.and_then(|i| layout.lines.get(i - 1)) else { return layout };
    let paint = deckcraft_model::edit::props_at(body, (last.para, last.end));
    deckcraft_model::edit::delete(body, (last.para, last.end), (usize::MAX, usize::MAX));
    let measure = |body: &TextBody, rect| {
        let shape = Shape { text: Some(body.clone()), ..Default::default() };
        deckcraft_text::layout_with_metrics(
            rctx,
            &shape,
            body,
            &deckcraft_text::Opts { rect, fields: &deckcraft_text::NoFields, prompt_color: None, no_shrink: true },
            deckcraft_text::LayoutMetrics { advance: deckcraft_text::AdvanceMetrics::Chart, line: deckcraft_text::LineMetrics::ChartLow },
        )
    };
    let mut kept = measure(body, rect);
    if policy != TextOverflow::Ellipsis {
        return kept;
    }
    let Some(last) = kept.lines.last() else { return kept };
    let mut marked_body = body.clone();
    deckcraft_model::edit::insert(&mut marked_body, (last.para, last.end), "…", Some(paint.clone()));
    let mut marked = measure(&marked_body, rect);
    let same_lines = marked.lines.len() == kept.lines.len()
        && marked.lines.iter().zip(&kept.lines).enumerate().all(|(i, (full, base))| {
            full.para == base.para && full.start == base.start && full.end == base.end.saturating_add(usize::from(i + 1 == kept.lines.len()))
        });
    if same_lines {
        let literal = marked.lines.iter().zip(&kept.lines).map(|(full, base)| {
            let width = base.extent.1 - base.extent.0;
            if full.rtl { (full.extent.1 - width, full.extent.1) } else { (full.extent.0, full.extent.0 + width) }
        });
        if let Some((left, right)) = literal.reduce(|a, b| (a.0.min(b.0), a.1.max(b.1))) {
            let dx = rect.center().x - (left + right) / 2.;
            for run in &mut marked.runs {
                for (_, x, _) in &mut run.glyphs {
                    *x += dx;
                }
            }
            for deco in &mut marked.decos {
                deco.rect = deco.rect + kurbo::Vec2::new(dx, 0.);
            }
            for line in &mut marked.lines {
                line.extent.0 += dx;
                line.extent.1 += dx;
                for x in &mut line.caret_x {
                    *x += dx;
                }
                for (left, right) in &mut line.edges {
                    *left += dx;
                    *right += dx;
                }
            }
        }
        return marked;
    }
    // If the authored wrapping changes a line break, keep the retained layout
    // and attach the marker without allowing another line past the height cap.
    let marker = label_body(
        "…",
        LabelStyle { size: paint.size.unwrap_or(12.), align: Align::Left, anchor: Anchor::Top, col: Rgba::BLACK, bold: false },
        &paint,
    );
    let mut mark = measure(&marker, Rect::new(0., 0., 1e6, 1e6));
    let dx = if last.rtl { last.extent.0 - mark.content_width } else { last.extent.1 };
    let dy = last.baseline - mark.lines.first().map_or(0., |line| line.baseline);
    for run in &mut mark.runs {
        run.para = last.para;
        run.chars = (last.end, last.end.saturating_add(1));
        for cell in &mut run.cells {
            cell.0 = last.end;
        }
        for (_, x, y) in &mut run.glyphs {
            *x += dx;
            *y += dy;
        }
    }
    for deco in &mut mark.decos {
        deco.para = last.para;
        deco.rect = deco.rect + kurbo::Vec2::new(dx, dy);
    }
    kept.runs.extend(mark.runs);
    kept.decos.extend(mark.decos);
    kept
}

/// Render a label through the same rich-text layout as native slide text.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn point_data_label(
    ctx: &mut RenderContext,
    rctx: &Ctx,
    m: Affine,
    text: &str,
    mut r: Rect,
    style: LabelStyle,
    source: (&Chart, &deckcraft_model::chart::Series, usize, Rect, (f64, f64)),
) {
    let (chart, series, index, mark, frame) = source;
    let spec = series.label(index);
    if spec.hidden == Some(true) {
        return;
    }
    // A literal c:tx/c:rich is a complete text body. Native charts do not blend the
    // separate txPr of their generated numeric label into that replacement body.
    let text_style = spec.style.as_ref().filter(|_| spec.text.is_none());
    let mut default_props = text_style.and_then(|b| b.paragraphs.first()).map(|p| p.def_props.clone()).unwrap_or_default();
    default_props.inherit(&chart.text_props(ChartTextRole::Default));
    default_props.size = default_props.size.or(chart.text_size);
    let (align, anchor) = (style.align, style.anchor);
    let automatic = label_body(text, style, &default_props);
    let mut body = spec.text.clone().unwrap_or_else(|| automatic.clone());
    let literal_anchor = body.body.anchor;
    if let Some(explicit) = text_style {
        body.body.inherit(&explicit.body);
    }
    let literal_column = spec.text.is_some() && chart.series_kind(series) == ChartType::Column;
    if literal_column {
        body.body.wrap.get_or_insert(true);
        // Chart-label text margins differ from ordinary slide text. Native filled-label
        // controls retain 3pt horizontally and 1.5pt vertically at every font size.
        body.body.inset_l.get_or_insert(3.);
        body.body.inset_r.get_or_insert(3.);
        body.body.inset_t.get_or_insert(1.5);
        body.body.inset_b.get_or_insert(1.5);
    }
    body.body.inherit(&automatic.body);
    let mut inherited = text_style.and_then(|b| b.paragraphs.first()).map(|p| p.def_props.clone()).unwrap_or_default();
    if let Some(p) = automatic.paragraphs.first().and_then(|p| p.runs.first()) {
        inherited.inherit(&p.props);
    }
    for (i, p) in body.paragraphs.iter_mut().enumerate() {
        if let Some(style) = text_style.and_then(|b| b.paragraphs.get(i).or_else(|| b.paragraphs.first())) {
            let mut props = style.props.clone();
            props.inherit(&p.props);
            p.props = props;
        }
        p.props.bullet = Some(Bullet::None);
        if let Some(a) = automatic.paragraphs.first() {
            p.props.inherit(&a.props);
        }
        p.def_props.inherit(&inherited);
        for run in &mut p.runs {
            run.props.inherit(&p.def_props);
        }
    }
    if literal_column
        && matches!(body.body.vert, None | Some(deckcraft_model::text::TextDir::Horizontal))
        && matches!(spec.position.as_deref(), None | Some("ctr" | "inBase" | "inEnd" | "outEnd"))
        && (body.body.rot.unwrap_or(0.).abs() != 180. || spec.position.as_deref() == Some("ctr"))
        && let Some(bounds) = label_layout::bounds(frame, body.body.rot.unwrap_or(0.), spec.layout.as_ref())
    {
        body.body.anchor = Some(literal_anchor.unwrap_or(Anchor::Middle));
        let negative = series.values.get(index).copied().flatten().is_some_and(|v| v < 0.);
        column_rich_label(ctx, rctx, m, body, &spec, bounds, mark, frame, negative);
        return;
    }
    // Measure the complete body, not each run separately: soft breaks, bidi joining,
    // superscripts, mixed fonts and paragraph spacing all affect the label's bounds.
    // Native automatic column labels cap wrapping at one fifth of chart width, regardless
    // of gap width or inside/outside placement. Explicit no-wrap and manual widths win.
    let wrap_width = if literal_column && body.body.wrap == Some(true) {
        spec.layout.as_ref().filter(|l| l.w.is_some()).map_or(frame.0 * 0.2, |l| l.resolve(r, frame.0, frame.1).width())
    } else {
        1e6
    };
    let wrap_width = wrap_width.clamp(1., 1e6);
    let automatic_column = literal_column
        && spec.layout.is_none()
        && body.body.rot.unwrap_or(0.) == 0.
        && matches!(body.body.vert, None | Some(deckcraft_model::text::TextDir::Horizontal));
    let mut measured = label_layout::measure(rctx, &body, wrap_width, automatic_column);
    let mut marker_base = None;
    if automatic_column {
        (measured, marker_base) = label_layout::overflow(rctx, &mut body, wrap_width, (frame.1 * 0.5).clamp(1., 1e6), measured);
    }
    let h = (measured.content_height + body.body.inset_t.unwrap_or(0.) + body.body.inset_b.unwrap_or(0.)).max(1.);
    // Explicit overflow retains the complete body, but automatic positioning still
    // uses the bounded height. Its trailing lines are clipped by the chart frame.
    let placement_h = if automatic_column && body.body.vert_overflow == Some(deckcraft_model::text::TextOverflow::Overflow) {
        h.min((frame.1 * 0.5).max(1.))
    } else {
        h
    };
    let w = (measured.content_width + body.body.inset_l.unwrap_or(0.) + body.body.inset_r.unwrap_or(0.) + 4.).clamp(1., wrap_width);
    let x = match align {
        Align::Left => r.x0,
        Align::Right => r.x1 - w,
        _ => r.center().x - w / 2.,
    };
    let y = match anchor {
        Anchor::Top => r.y0,
        Anchor::Bottom => r.y1 - placement_h,
        _ => r.center().y - placement_h / 2.,
    };
    r = Rect::new(x, y, x + w, y + h);
    match spec.position.as_deref() {
        Some("ctr" | "inBase" | "inEnd") => {
            let mut p = mark.center();
            let negative = series.values.get(index).copied().flatten().is_some_and(|v| v < 0.);
            let end = spec.position.as_deref() == Some("inEnd");
            if matches!(spec.position.as_deref(), Some("inBase" | "inEnd")) {
                if chart.series_kind(series).horizontal() {
                    p.x = if end != negative { mark.x1 - w / 2. - 2. } else { mark.x0 + w / 2. + 2. };
                } else {
                    p.y = if end != negative { mark.y0 + placement_h / 2. + 2. } else { mark.y1 - placement_h / 2. - 2. };
                }
            }
            r = Rect::new(p.x - w / 2.0, p.y - placement_h / 2.0, p.x + w / 2.0, p.y - placement_h / 2.0 + h);
            body.body.anchor = Some(Anchor::Middle);
        }
        Some("b") => {
            r = Rect::new(mark.center().x - w / 2.0, mark.y1 + 2.0, mark.center().x + w / 2.0, mark.y1 + 2.0 + h);
        }
        Some("l") => {
            r = Rect::new(mark.x0 - w - 2.0, mark.center().y - h / 2.0, mark.x0 - 2.0, mark.center().y + h / 2.0);
        }
        Some("r") => {
            r = Rect::new(mark.x1 + 2.0, mark.center().y - h / 2.0, mark.x1 + w + 2.0, mark.center().y + h / 2.0);
        }
        _ => {}
    }
    // Automatic outside labels clear their rotated bounding box, not just the unrotated
    // text height. Explicit manual placement remains the author's choice.
    if spec.layout.is_none() && matches!(spec.position.as_deref(), None | Some("outEnd" | "t")) && r.y1 <= mark.y0 + 1e-6 {
        let angle = body.body.rot.unwrap_or(0.).to_radians();
        let extra = (r.width() * angle.sin().abs() + r.height() * angle.cos().abs() - r.height()).max(0.) / 2.;
        r = r + kurbo::Vec2::new(0., -extra);
    }
    if let Some(layout) = &spec.layout {
        r = layout.resolve(r, frame.0, frame.1);
    }
    if automatic_column {
        // Automatic labels remain inside their chart even when outside-end placement
        // would put the top above it. Author-positioned labels keep their coordinates.
        let x = r.x0.clamp(0., (frame.0 - r.width()).max(0.));
        let y = r.y0.clamp(0., (frame.1 - placement_h).max(0.));
        r = r + kurbo::Vec2::new(x - r.x0, y - r.y0);
        ctx.set_transform(m);
        ctx.push_clip_layer(&Rect::new(0., 0., frame.0, frame.1).to_path(0.1));
    }
    if let Some(fill) = &spec.fill {
        crate::paint::fill_path(ctx, rctx, fill, None, &r.to_path(0.1), r, m, deckcraft_geom::preset::FillMode::Norm, 1.0, rctx.pres);
    }
    if let Some(line) = &spec.line {
        chart_stroke(ctx, rctx, m, &r.to_path(0.1), Some(line), Rgba::BLACK);
    }
    let layout = label_layout::placed(rctx, &body, r, marker_base.as_ref(), automatic_column);
    crate::draw_layout(ctx, &layout, r, m);
    if automatic_column {
        ctx.pop_layer();
    }
}

#[allow(clippy::too_many_arguments)]
fn column_rich_label(
    ctx: &mut RenderContext,
    rctx: &Ctx,
    m: Affine,
    mut body: TextBody,
    spec: &deckcraft_model::chart::DataLabel,
    bounds: label_layout::Bounds,
    mark: Rect,
    frame: (f64, f64),
    negative: bool,
) {
    if bounds.width <= 0. || bounds.height <= 0. {
        return;
    }
    let custom_insets = label_layout::custom_column_insets(&body);
    label_layout::column_vertical_insets(&mut body);
    let angle = body.body.rot.take().unwrap_or(0.);
    // Chart dimensions describe the box before rotation, including quarter turns.
    // Slide text's quarter-turn dimension swap must not be applied a second time.
    body.body.rot = Some(0.);
    let center_manual = bounds.fixed && label_layout::manual_alignment(&mut body);
    let wrap = body.body.wrap == Some(true);
    let half_turn = angle.abs() == 180.;
    let minimal_width = half_turn && custom_insets && wrap && label_layout::bounded_minimal_measurement(&body);
    if half_turn {
        // Half turns retain the complete body. Custom margins measure wrapped
        // text at minimal width, then paint again at the resulting natural width;
        // thin letters can therefore share a painted line inside a taller box.
        body.body.wrap = Some(minimal_width);
        body.body.vert_overflow = Some(deckcraft_model::text::TextOverflow::Overflow);
    }
    let wrap_width = if minimal_width {
        0.
    } else if wrap && !half_turn {
        bounds.width
    } else {
        1e6
    };
    let measured = label_layout::measure(rctx, &body, wrap_width, true);
    let (measured, marker_base) = label_layout::overflow(rctx, &mut body, wrap_width, bounds.height, measured);
    let overflow = body.body.vert_overflow == Some(deckcraft_model::text::TextOverflow::Overflow);
    let text_h = (measured.content_height + body.body.inset_t.unwrap_or(0.) + body.body.inset_b.unwrap_or(0.)).max(1.);
    let text_w = (measured.content_width + body.body.inset_l.unwrap_or(0.) + body.body.inset_r.unwrap_or(0.))
        .clamp(1., if minimal_width { 1e6 } else { wrap_width.max(1.) });
    let (mut w, h) = if bounds.fixed { (bounds.width, bounds.height) } else { (text_w, text_h) };
    let marked_w = w;
    let mut background_x = 0.;
    if !bounds.fixed && marker_base.is_some() {
        let preview = label_layout::placed(rctx, &body, Rect::new(0., 0., w, h), marker_base.as_ref(), true);
        if let Some((left, right)) = label_layout::background_extent(&preview) {
            let width = right - left + body.body.inset_l.unwrap_or(0.) + body.body.inset_r.unwrap_or(0.);
            if width.is_finite() && width > 0. && width <= wrap_width {
                background_x = left - body.body.inset_l.unwrap_or(0.);
                w = width.max(1.);
            }
        }
    }
    // Custom margins center the full overflowing body, using the capped height
    // only for its baseline constraint. Default margins keep the leading-corner
    // policy except for horizontal no-wrap overflow. The chart clip still applies.
    let uncapped = !bounds.fixed
        && spec.layout.is_none()
        && overflow
        && (custom_insets || (!wrap && (angle == 0. || half_turn)))
        && spec.position.as_deref() == Some("ctr");
    let mut r = label_layout::position((w, h), angle, mark, spec.position.as_deref(), negative, frame, spec.layout.as_ref(), bounds.fixed, !uncapped);
    if !bounds.fixed && (overflow || text_h > bounds.height) && !uncapped {
        // Explicit overflow and an oversized retained first line keep the full box.
        // Position its leading corner using the capped outer bounds, then let the
        // content extend toward the trailing edges, including under clip/ellipsis.
        let outer = label_layout::rotated_extent(w, h, angle);
        let cap = (outer.0.min(frame.0 / 5.), outer.1.min(frame.1 / 2.));
        let bounded = label_layout::position(cap, 0., mark, spec.position.as_deref(), negative, frame, spec.layout.as_ref(), false, true);
        let center = Point::new(bounded.x0 + outer.0 / 2., bounded.y0 + outer.1 / 2.);
        r = Rect::new(center.x - w / 2., center.y - h / 2., center.x + w / 2., center.y + h / 2.);
    }
    let center = r.center().to_vec2();
    let transform = m * Affine::translate(center) * Affine::rotate(angle.to_radians()) * Affine::translate(-center);
    ctx.set_transform(m);
    ctx.push_clip_layer(&Rect::new(0., 0., frame.0, frame.1).to_path(0.1));
    if let Some(fill) = &spec.fill {
        crate::paint::fill_path(ctx, rctx, fill, None, &r.to_path(0.1), r, transform, deckcraft_geom::preset::FillMode::Norm, 1.0, rctx.pres);
    }
    if let Some(line) = &spec.line {
        chart_stroke(ctx, rctx, transform, &r.to_path(0.1), Some(line), Rgba::BLACK);
    }
    let clip_text = !overflow && (bounds.fixed || h > bounds.height);
    if clip_text {
        // An oversized automatic first line retains its full background, but its
        // ink is clipped to the height budget centered on that box. Both automatic
        // and manual clips permit horizontal ink overflow, including the marker.
        let reach = frame.0 + frame.1 + w;
        let half_height = bounds.height.min(h) / 2.;
        ctx.set_transform(transform);
        ctx.push_clip_layer(&Rect::new(r.x0 - reach, r.center().y - half_height, r.x1 + reach, r.center().y + half_height).to_path(0.1));
    }
    let mut text_rect = r;
    if !bounds.fixed && marker_base.is_some() {
        text_rect.x0 -= background_x;
        text_rect.x1 = text_rect.x0 + marked_w;
    }
    if bounds.fixed && overflow && text_h > h {
        // Overflow anchors the full text, including a negative top offset for center
        // and bottom anchors. Ordinary slide layout clamps that offset to zero.
        let extra = text_h - h;
        let offset = match body.body.anchor {
            Some(Anchor::Middle) => extra / 2.,
            Some(Anchor::Bottom) => extra,
            _ => 0.,
        };
        text_rect = Rect::new(r.x0, r.y0 - offset, r.x1, r.y0 - offset + text_h);
    }
    let layout = label_layout::placed(rctx, &body, text_rect, marker_base.as_ref(), true);
    let text_transform =
        if center_manual { transform * Affine::translate((label_layout::centering_offset(&layout, &body, text_rect), 0.)) } else { transform };
    crate::draw_layout(ctx, &layout, text_rect, text_transform);
    if clip_text {
        ctx.pop_layer();
    }
    ctx.pop_layer();
}

fn merged_chart_body(explicit: &deckcraft_model::text::BodyProps, automatic: &deckcraft_model::text::BodyProps) -> deckcraft_model::text::BodyProps {
    let mut props = explicit.clone();
    props.inherit(automatic);
    props
}

/// Apply the complete DrawingML line style; absent styles retain the automatic colour.
fn chart_stroke(ctx: &mut RenderContext, rctx: &Ctx, m: Affine, path: &BezPath, line: Option<&deckcraft_model::Line>, fallback: Rgba) {
    let mut line = line.cloned().unwrap_or_default();
    if matches!(line.fill, Some(Fill::None)) {
        return;
    }
    line.fill.get_or_insert_with(|| Fill::solid(ColorRef::rgb(fallback)));
    crate::stroke::stroke(ctx, rctx, &line, None, path, None, m, line.width.unwrap_or(0.75).clamp(0.0, 2000.0), 1.0);
}

/// Merge overlapping flat grid strokes before antialiasing their common ends.
/// The primary tick budget bounds the input to1001 segments and the union to
/// at most5005 path elements. Unsupported line styles keep the stroke path.
fn fill_overlapping_grid(
    ctx: &mut RenderContext,
    rctx: &Ctx,
    m: Affine,
    path: &BezPath,
    line: Option<&deckcraft_model::Line>,
    fallback: Rgba,
) -> bool {
    use deckcraft_model::style::{Compound, LineCap};
    let line = line.cloned().unwrap_or_default();
    if path.elements().len() > 2002
        || !path.elements().len().is_multiple_of(2)
        || line.dash.is_some()
        || line.cap.unwrap_or_default() != LineCap::Flat
        || line.compound.unwrap_or_default() != Compound::Single
    {
        return false;
    }
    let paint = match &line.fill {
        None => fallback,
        Some(Fill::Solid { color }) => rctx.color(color, None),
        _ => return false,
    };
    let width = line.width.unwrap_or(0.75).clamp(0.0, 2000.0).max(0.5 / scale_of(m).max(1e-6));
    if !width.is_finite() {
        return false;
    }
    let mut union = BezPath::new();
    let mut pending: Option<Rect> = None;
    let mut horizontal = None;
    let mut merged = false;
    for segment in path.elements().chunks_exact(2) {
        let [kurbo::PathEl::MoveTo(a), kurbo::PathEl::LineTo(b)] = segment else { return false };
        let horiz = a.y == b.y;
        if !horiz && a.x != b.x || horizontal.is_some_and(|h| h != horiz) {
            return false;
        }
        horizontal = Some(horiz);
        let next = if horiz {
            Rect::new(a.x.min(b.x), a.y - width / 2., a.x.max(b.x), a.y + width / 2.)
        } else {
            Rect::new(a.x - width / 2., a.y.min(b.y), a.x + width / 2., a.y.max(b.y))
        };
        if let Some(previous) = pending {
            let same_ends = if horiz { previous.x0 == next.x0 && previous.x1 == next.x1 } else { previous.y0 == next.y0 && previous.y1 == next.y1 };
            if !same_ends {
                return false;
            }
            let touches = if horiz { next.y0 <= previous.y1 && previous.y0 <= next.y1 } else { next.x0 <= previous.x1 && previous.x0 <= next.x1 };
            if touches {
                pending = Some(previous.union(next));
                merged = true;
                continue;
            }
            union.extend(previous.to_path(0.1));
        }
        pending = Some(next);
    }
    if !merged {
        return false;
    }
    if let Some(last) = pending {
        union.extend(last.to_path(0.1));
    }
    ctx.set_transform(m);
    ctx.set_fill_rule(vello_cpu::peniko::Fill::NonZero);
    ctx.set_paint(color(paint, 1.0));
    ctx.fill_path(&union);
    true
}

/// Measure with the exact body/style used for drawing, including CJK, fallback and shaping.
fn label_width(ctx: &Ctx, text: &str, size: f64, props: &RunProps) -> f64 {
    let body = label_body(text, LabelStyle { size, align: Align::Left, anchor: Anchor::Top, col: Rgba::BLACK, bold: false }, props);
    let shape = Shape { text: Some(body.clone()), ..Default::default() };
    deckcraft_text::layout(
        ctx,
        &shape,
        &body,
        &deckcraft_text::Opts {
            rect: Rect::new(0.0, 0.0, 1_000_000.0, 1_000_000.0),
            fields: &deckcraft_text::NoFields,
            prompt_color: None,
            no_shrink: true,
        },
    )
    .content_width
}

fn axis_font_size(c: &Chart, role: ChartTextRole, base: f64) -> f64 {
    c.text_props(role).size.unwrap_or(base).clamp(1., 4000.)
}

struct AxisMetrics {
    height: f64,
    ascent: f64,
    spaced_height: f64,
}

fn axis_metrics(ctx: &Ctx, c: &Chart, role: ChartTextRole, base: f64) -> Option<AxisMetrics> {
    let size = axis_font_size(c, role, base);
    let mut body = label_body("0", LabelStyle { size, align: Align::Left, anchor: Anchor::Top, col: Rgba::BLACK, bold: false }, &c.text_props(role));
    let natural = label_layout::measure(ctx, &body, 1e6, true);
    let line = natural.lines.first()?;
    let height = line.bottom - line.top;
    let ascent = line.baseline - line.top;
    let spaced_height = if let Some(spacing) = c.line_spacing(role) {
        for p in &mut body.paragraphs {
            p.props.line_spacing = Some(*spacing);
        }
        let spaced = label_layout::measure(ctx, &body, 1e6, true);
        spaced.lines.first().map_or(height, |l| l.bottom - l.top)
    } else {
        height
    };
    [height, ascent, spaced_height].iter().all(|v| v.is_finite() && *v >= 0.).then_some(AxisMetrics { height, ascent, spaced_height })
}

/// The value gutter measures exact bounds and their major-unit neighbors, even
/// when those samples are not painted ticks or lie beyond a very short range.
/// Measuring every emitted tick instead changes the plot with interior digits.
fn linear_axis_reserve_width(ctx: &Ctx, c: &Chart, (lo, hi, step): (f64, f64, f64), size: f64) -> Option<f64> {
    let samples = [lo, lo + step, hi - step, hi];
    if !step.is_finite() || step <= 0. || hi < lo || samples.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let mut props = c.text_props(ChartTextRole::ValueAxis);
    // The axis reserve uses unkerned advances, independently of authored text
    // thresholds and the slide's ordinary text style. Keep the model unchanged.
    props.kern = Some(0.);
    Some(samples.into_iter().map(|v| label_width(ctx, &c.axis_label(false, v), size, &props)).fold(0., f64::max))
}

/// Axis text reserves use font metrics, independently of the chart's dimensions or
/// default font. These controls cover undecorated Columns with horizontal axis text.
struct ColumnAxisPlot {
    rect: Rect,
    category_offset: f64,
    value_reserve_active: bool,
}

struct ColumnAxisStage {
    scale: (f64, f64, f64),
    initial_right: Option<f64>,
}

fn column_axis_stage(ctx: &Ctx, c: &Chart, area: Rect, frame: (f64, f64), base: f64, final_scale: (f64, f64, f64)) -> Option<ColumnAxisStage> {
    if c.kind != ChartType::Column || authored_inner_plot(c).is_none() {
        return None;
    }
    let scale = c.initial_authored_axis_scale(false)?;
    let width = linear_axis_reserve_width(ctx, c, scale, axis_font_size(c, ChartTextRole::ValueAxis, base))?;
    let plot = column_axis_plot(ctx, c, area, frame, base, width, scale.0)?;
    Some(ColumnAxisStage {
        scale: if plot.value_reserve_active { final_scale } else { scale },
        initial_right: plot.value_reserve_active.then_some(plot.rect.x1),
    })
}

fn column_axis_plot(ctx: &Ctx, c: &Chart, area: Rect, frame: (f64, f64), base: f64, value_width: f64, axis_min: f64) -> Option<ColumnAxisPlot> {
    let (w, h) = frame;
    if c.kind != ChartType::Column
        || c.title.is_some()
        || c.legend.is_some()
        || c.axis_titles != (None, None)
        || c.series.iter().any(|s| c.on_secondary(s))
        || ![w, h, value_width].iter().all(|v| v.is_finite() && *v >= 0.)
        || [ChartTextRole::CategoryAxis, ChartTextRole::ValueAxis].iter().any(|role| {
            c.format.bodies.get(role).is_some_and(|b| {
                b.rot.unwrap_or(0.) != 0.
                    || !matches!(b.vert, None | Some(deckcraft_model::text::TextDir::Horizontal))
                    || b.columns.unwrap_or(1) > 1
                    || b.warp.is_some()
            })
        })
    {
        return None;
    }
    let outer = c.format.plot_layout.as_ref().is_some_and(|l| !l.inner);
    let inner = authored_inner_plot(c);
    let rect = if outer {
        area
    } else if inner.is_some() {
        Rect::new(0., 0., w, h)
    } else {
        Rect::new(5_f64.min(w / 2.), 5_f64.min(h / 2.), (w - 5.).max(w / 2.), (h - 5.).max(h / 2.))
    };
    let floor = if outer || inner.is_some() { 0. } else { 6. };
    let value = axis_metrics(ctx, c, ChartTextRole::ValueAxis, base)?;
    let left = if c.y_hidden || c.y_tick_labels_hidden { 0. } else { 1.5 + value_width + value.height / 2. + value.ascent / 3. };
    let value_end = if c.y_hidden || c.y_tick_labels_hidden { 0. } else { value.spaced_height / 2. };
    let x0 = (rect.x0 + left.max(floor)).min(rect.x1);
    let x1 = (rect.x1 - floor).max(x0);
    let label_width = inner.map_or(x1 - x0, |l| l.resolve(rect, w, h).width().max(0.).min(x1 - x0));
    let (extra, offset) = category_layout_adjustment(ctx, c, base, label_width, false);
    let category = axis_metrics(ctx, c, ChartTextRole::CategoryAxis, base)?;
    let reserve = 1.5 + category.height + 2. * category.ascent / 3. + extra;
    let (low, high) = category_label_edges(c, axis_min);
    let top = if high { reserve.max(value_end) } else { value_end };
    let bottom = if low { reserve.max(value_end) } else { value_end };
    let mut y0 = (rect.y0 + top.max(floor)).min(rect.y1);
    let mut y1 = (rect.y1 - bottom.max(floor)).max(y0);
    if inner.is_none() && (low || high) {
        // Automatic and outer layouts share the remaining allocation between
        // plot and category text. Extra category lines cannot consume that
        // minimum plot allocation; both value-end reserves still bound it.
        let end = value_end.max(floor);
        let compact = rect.height() / 2. - 1.5 - 2. * category.ascent / 3. - offset;
        let extent = (y1 - y0).max(compact).min(rect.height() - 2. * end).max(10. / deckcraft_geom::EMU_PER_PT);
        if low {
            y0 = (rect.y0 + end).min(rect.y1);
            y1 = y0 + extent;
        } else {
            y1 = (rect.y1 - end).max(rect.y0);
            y0 = y1 - extent;
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let bounds = Rect::new(x0, y0, x1, y1);
    let plot = inner.map_or(bounds, |l| {
        let mut layout = l.clone();
        // Factor positions offset the centered authored extent. An absent x
        // also suppresses the y offset, retaining its edge or centered origin.
        // These are working allocations; keep the authored model unchanged.
        if layout.x.is_none() {
            layout.y = layout.y.and(if layout.y_edge { None } else { Some(0.) });
        }
        let origin = |position: Option<f64>, extent: Option<f64>, edge: bool, extent_edge: bool, frame: f64, default: f64| {
            if position.is_some() && !edge && !extent_edge {
                extent.filter(|v| v.is_finite() && (0.0..=1.0).contains(v)).map_or(default, |v| (1. - v) * frame / 2.)
            } else {
                default
            }
        };
        let x = origin(layout.x, layout.w, layout.x_edge, layout.w_edge, w, bounds.x0);
        let y = origin(layout.y, layout.h, layout.y_edge, layout.h_edge, h, bounds.y0);
        let automatic = Rect::new(x, y, x + bounds.width(), y + bounds.height());
        let mut plot = fit_inner_plot(layout.resolve(automatic, w, h), bounds, (value_end, h - value_end));
        // A complete authored rectangle can place category text outside the
        // frame. Incomplete layouts also constrain that automatic allocation
        // to the category reserve, without moving an already fitted origin.
        if [l.x, l.y, l.w, l.h].iter().any(Option::is_none) {
            plot.y0 = plot.y0.clamp(bounds.y0, bounds.y1);
            plot.y1 = plot.y1.min(bounds.y1).max(plot.y0);
        }
        plot
    });
    Some(ColumnAxisPlot { rect: plot, category_offset: offset, value_reserve_active: !c.y_hidden && !c.y_tick_labels_hidden && plot.x0 <= bounds.x0 })
}

/// Preserve authored size while translating into the space left by tick text.
/// When the two reserves leave less space, reduce only the constrained extent.
fn fit_inner_plot(requested: Rect, bounds: Rect, vertical: (f64, f64)) -> Rect {
    let fit = |start: f64, end: f64, low: f64, high: f64| {
        let extent = (end - start).max(0.).min((high - low).max(0.));
        let start = start.max(low).min(high - extent);
        (start, start + extent)
    };
    let (x0, x1) = fit(requested.x0, requested.x1, bounds.x0, bounds.x1);
    // An authored interval may place category text outside the chart. Preserve
    // that placement when the value ticks fit; use the category reserve only
    // when an end collision requires the vertical interval to be fitted again.
    let (y0, y1) = if requested.y0 >= vertical.0 && requested.y1 <= vertical.1 {
        (requested.y0, requested.y1)
    } else {
        fit(requested.y0, requested.y1, bounds.y0, bounds.y1)
    };
    Rect::new(x0, y0, x1, y1)
}

fn authored_inner_plot(c: &Chart) -> Option<&ChartLayout> {
    c.format.plot_layout.as_ref().filter(|l| {
        let fields = [l.x, l.y, l.w, l.h];
        // Empty manual layouts remain automatic; any authored coordinate
        // selects the complete inner frame before tick reserves are applied.
        l.inner && fields.iter().any(Option::is_some)
    })
}

fn category_label_edges(c: &Chart, axis_min: f64) -> (bool, bool) {
    if c.cat_hidden || c.cat_deleted {
        return (false, false);
    }
    match c.cat_label_position {
        Some(ChartTickPosition::Low) => (true, false),
        Some(ChartTickPosition::High) => (false, true),
        _ => match c.cat_crossing {
            Some(ChartAxisCrossing::Min) => (true, false),
            Some(ChartAxisCrossing::Max) => (false, true),
            Some(ChartAxisCrossing::Value(v)) if v.is_finite() => (v <= axis_min, false),
            _ => (0. <= axis_min, false),
        },
    }
}

/// Authored category line spacing and label distance change the required gutter.
/// The default gutter is still approximate; these deltas are independent of it.
fn category_layout_adjustment(ctx: &Ctx, c: &Chart, base: f64, width: f64, horiz: bool) -> (f64, f64) {
    if horiz || matches!(c.kind, ChartType::Scatter | ChartType::Bubble) || c.cat_hidden || c.cat_deleted || c.categories.is_empty() {
        return (0., 0.);
    }
    let props = c.text_props(ChartTextRole::CategoryAxis);
    let size = props.size.unwrap_or(base).clamp(1., 4000.);
    let height = |text: &str, spacing: bool| {
        let mut body = label_body(text, LabelStyle { size, align: Align::Left, anchor: Anchor::Top, col: Rgba::BLACK, bold: false }, &props);
        if spacing {
            for p in &mut body.paragraphs {
                p.props.line_spacing = c.line_spacing(ChartTextRole::CategoryAxis).cloned();
            }
        }
        // Exact line spacing reserves the whole last line, including its trailing
        // part. Slide-text content_height ends at the last descent instead.
        label_layout::measure(ctx, &body, 1e6, true).lines.iter().map(|l| l.bottom - l.top).sum::<f64>()
    };
    let natural = height("0", false);
    let offset = (f64::from(c.cat_label_offset.unwrap_or(100).min(1000)) - 100.) * natural / 400.;
    // Bound this additional measurement pass even when the chart cache is huge.
    if c.categories.len() > 256 || c.categories.iter().try_fold(0usize, |n, text| n.checked_add(text.len())).is_none_or(|n| n > 65_536) {
        return (offset, offset);
    }
    let count = c.categories.len().max(c.series.iter().map(|s| s.values.len()).max().unwrap_or(0)).max(1);
    let (back, forward) = forecast_pad(c);
    let band = width / (count as f64 + back + forward);
    let wraps = c.categories.iter().any(|text| label_width(ctx, text, size, &props) > band);
    let maximum = c.categories.iter().map(|text| height(&if wraps { split_label(text) } else { text.clone() }, true)).fold(0., f64::max);
    (maximum - natural + offset, offset)
}

fn hidden_axis_plot(c: &Chart, area: Rect, w: f64, h: f64) -> Option<Rect> {
    if c.kind != ChartType::Column
        || !(c.cat_hidden || c.cat_deleted)
        || !(c.y_hidden || c.y_tick_labels_hidden)
        || c.title.is_some()
        || c.legend.is_some()
        || c.axis_titles != (None, None)
    {
        return None;
    }
    // With no axis labels or other chart decorations, native Column controls
    // retain a fixed point margin across chart dimensions and axis fonts. A
    // supplied outer plot already includes that frame and needs no reserve.
    if c.format.plot_layout.as_ref().is_some_and(|l| !l.inner) {
        Some(area)
    } else {
        let x = 11_f64.min(w / 2.);
        let y = 11_f64.min(h / 2.);
        Some(Rect::new(x, y, w - x, h - y))
    }
}

fn fmt_num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 { format!("{}", v.round() as i64) } else { format!("{v:.1}") }
}

/// A scatter or bubble chart's value x axis (min, max, major unit): one scale over every series'
/// x values, from zero when they're all positive. `None` for other charts and when there are no x values.
fn value_x_scale(c: &Chart, max_intervals: usize) -> Option<(f64, f64, f64)> {
    if c.series.is_empty() || !c.series.iter().all(|s| matches!(c.series_kind(s), ChartType::Scatter | ChartType::Bubble)) {
        return None;
    }
    let xs =
        c.series.iter().flat_map(|s| {
            let n = if s.x_values.is_empty() { s.values.len() } else { s.x_values.len() };
            (0..n).filter_map(move |i| {
                if s.x_values.is_empty() { Some(i as f64 + 1.0) } else { s.x_values.get(i).copied().flatten().filter(|v| v.is_finite()) }
            })
        });
    let (lo, hi) = xs.fold(None, |r: Option<(f64, f64)>, v| Some(r.map_or((v, v), |(a, b)| (a.min(v), b.max(v)))))?;
    let (lo, hi) = if c.kind == ChartType::Bubble { bubble_extent(lo.min(0.0), hi.max(0.0), 0.2) } else { (lo, hi) };
    Some(axis_scale(lo.min(0.0), hi, None, None, None, max_intervals, false))
}

/// Leave room for the glyph, rather than placing its centre on the plot border.
fn bubble_extent(lo: f64, hi: f64, fraction: f64) -> (f64, f64) {
    let pad = (hi - lo).abs().max(hi.abs() * 0.1).max(f64::MIN_POSITIVE) * fraction;
    let padded = (if lo == 0.0 { 0.0 } else { lo - pad }, if hi == 0.0 { 0.0 } else { hi + pad });
    if padded.0.is_finite() && padded.1.is_finite() { padded } else { (lo, hi) }
}

fn primary_scale(c: &Chart, max_intervals: usize) -> (f64, f64, f64) {
    let scale = c.axis_scale(false, max_intervals).unwrap_or_else(|| axis_scale(0.0, 1.0, None, None, None, max_intervals, true));
    if c.kind != ChartType::Bubble {
        return scale;
    }
    let (lo, hi) = c
        .series
        .iter()
        .flat_map(|s| s.values.iter().flatten().copied())
        .filter(|v| v.is_finite())
        .fold((0.0_f64, 0.0_f64), |(lo, hi), v| (lo.min(v), hi.max(v)));
    let (lo, hi) = bubble_extent(lo, hi, 0.125);
    axis_scale(lo, hi, c.y_min, c.y_max, c.y_major, max_intervals, false)
}

/// Where the legend goes for `c:legendPos`, and the plot area left over.
/// `size` is the legend block (width, height); the result is its top-left corner and the plot area.
/// `r` (also the OOXML default and the fallback for unknown values) and `l` are vertical stacks centred
/// on the area, `tr` is a vertical stack in the top-right corner, `t` and `b` are centred rows.
/// An overlaid legend leaves the plot area as it was.
fn legend_layout(pos: &str, size: (f64, f64), area: Rect, base: f64, overlay: bool) -> (Point, Rect) {
    let (lw, lh) = size;
    let gap = base * 0.5;
    let cx = (area.x0 + area.x1) / 2.0;
    let cy = ((area.y0 + area.y1) / 2.0 - lh / 2.0).max(area.y0);
    let mut plot = area;
    let origin = match pos {
        "b" => {
            plot.y1 = area.y1 - lh - gap;
            Point::new(cx - lw / 2.0, area.y1 - lh)
        }
        "t" => {
            plot.y0 = area.y0 + lh + gap;
            Point::new(cx - lw / 2.0, area.y0)
        }
        "l" => {
            plot.x0 = area.x0 + lw + gap;
            Point::new(area.x0, cy)
        }
        "tr" => {
            plot.x1 = area.x1 - lw - gap;
            Point::new(area.x1 - lw, area.y0)
        }
        _ => {
            plot.x1 = area.x1 - lw - gap;
            Point::new(area.x1 - lw, cy)
        }
    };
    (origin, if overlay { area } else { plot })
}

/// An axis span to divide by: the span itself when positive, else 1 (no absolute floor, so tiny data keeps its scale).
pub(crate) fn positive_span(s: f64) -> f64 {
    if s > 0.0 && s.is_finite() { s } else { 1.0 }
}

/// The in-range tick values of an axis. The work cap matches the model's
/// accepted authored interval budget; it must not truncate ordinary dense axes.
fn axis_ticks((lo, hi, st): (f64, f64, f64)) -> impl Iterator<Item = f64> {
    let intervals = (hi - lo) / st;
    let count = if lo.is_finite() && hi.is_finite() && st.is_finite() && st > 0.0 && intervals.is_finite() && intervals >= 0.0 {
        (intervals + 1e-9).floor().clamp(0.0, 1000.0) as usize + 1
    } else {
        0
    };
    (0..count).map(move |k| lo + k as f64 * st)
}

#[cfg(test)]
pub fn draw(ctx: &mut RenderContext, rctx: &Ctx, c: &Chart, m: Affine, w: f64, h: f64) {
    draw_with_view(ctx, rctx, c, m, w, h, Affine::IDENTITY);
}

pub fn draw_with_view(ctx: &mut RenderContext, rctx: &Ctx, c: &Chart, m: Affine, w: f64, h: f64, view: Affine) {
    let column_line = |kind| {
        matches!(
            kind,
            ChartType::Column
                | ChartType::StackedColumn
                | ChartType::PercentColumn
                | ChartType::Line
                | ChartType::LineMarkers
                | ChartType::StackedLine
        )
    };
    // These Cartesian series share axis and label layers. Other chart families keep their
    // existing painting order until their native mark/axis relationship is established.
    let axis_layers = column_line(c.kind);
    match c.kind {
        // Histograms and Pareto charts are drawn as the column (and line) chart of their bins.
        ChartType::Histogram | ChartType::Pareto => {
            draw_inner(ctx, rctx, &c.histogram_view(), ChartFrame { transform: m, view, width: w, height: h, boxes: None, axis_layers })
        }
        ChartType::BoxWhisker => {
            let (box_chart, boxes) = crate::chartex::box_view(c);
            draw_inner(ctx, rctx, &box_chart, ChartFrame { transform: m, view, width: w, height: h, boxes: Some(&boxes), axis_layers });
        }
        _ => draw_inner(ctx, rctx, c, ChartFrame { transform: m, view, width: w, height: h, boxes: None, axis_layers }),
    }
}

struct ChartFrame<'a> {
    transform: Affine,
    view: Affine,
    width: f64,
    height: f64,
    boxes: Option<&'a crate::chartex::Boxes>,
    axis_layers: bool,
}

/// PowerPoint's default chart text colour: `tx1` with `lumMod` 65000 and `lumOff` 35000, which
/// is 595959 on a light theme and a light grey on a dark one.
pub fn default_text_color() -> ColorRef {
    use deckcraft_color::ColorTransform;
    ColorRef::scheme(SchemeSlot::Tx1).with(ColorTransform::LumMod(65000)).with(ColorTransform::LumOff(35000))
}

fn draw_inner(ctx: &mut RenderContext, rctx: &Ctx, c: &Chart, frame: ChartFrame<'_>) {
    let ChartFrame { transform: m, view, width: w, height: h, boxes, axis_layers } = frame;
    // Text without a colour of its own takes PowerPoint's default, the theme's text colour at
    // lumMod 65 % / lumOff 35 % (595959 on a light theme, light grey on a dark one), so a chart
    // on a dark theme is readable (a fixed grey used to vanish there).
    let text_col = rctx.color(c.text_color.as_ref().unwrap_or(&default_text_color()), None);
    let grid_col = Rgba::rgb(0xD9, 0xD9, 0xD9);
    let automatic_dark = (axis_layers && c.style.clamp(1, 48) >= 41).then(|| Fill::solid(ColorRef::scheme(SchemeSlot::Dk1)));
    if let Some(f) = c.chart_fill.as_ref().or(automatic_dark.as_ref()) {
        crate::paint::fill_path(
            ctx,
            rctx,
            f,
            None,
            &Rect::new(0.0, 0.0, w, h).to_path(0.1),
            Rect::new(0.0, 0.0, w, h),
            m,
            deckcraft_geom::preset::FillMode::Norm,
            1.0,
            rctx.pres,
        );
    }
    // The file's text sizes when it has them (PowerPoint draws chart text at them); otherwise a size
    // that suits the chart.
    let base = c.text_props(ChartTextRole::Default).size.or(c.text_size).unwrap_or_else(|| (h.min(w) / 22.0).clamp(7.0, 18.0));
    let title_size = c.text_props(ChartTextRole::Title).size.unwrap_or(base * 1.4);
    let mut area = Rect::new(w * 0.04, h * 0.04, w * 0.96, h * 0.96);
    if c.kind == ChartType::RegionMap {
        // Native map controls retain this point inset across chart sizes and text sizes.
        const INSET: f64 = 5.775;
        area = Rect::new(INSET, INSET, (w - INSET).max(INSET), (h - INSET).max(INSET));
    }
    if let Some(t) = &c.title {
        let width = (label_width(rctx, t, title_size, &c.text_props(ChartTextRole::Title)) + title_size * 0.4).min(w);
        let auto = Rect::new((w - width) / 2.0, area.y0, (w + width) / 2.0, area.y0 + title_size * 1.3);
        let rect = c.format.layouts.get(&ChartTextRole::Title).map_or(auto, |l| {
            // Office sizes titles to their text; w/h only size plots and legends.
            let l = deckcraft_model::chart::ChartLayout { w: None, h: None, ..l.clone() };
            l.resolve(auto, w, h)
        });
        chart_label(
            ctx,
            rctx,
            m,
            t,
            rect,
            LabelStyle { size: title_size, align: Align::Center, anchor: Anchor::Top, col: text_col, bold: false },
            (c, ChartTextRole::Title, false),
        );
        if !c.format.title_overlay {
            area.y0 += title_size * 1.7;
        }
    }
    let pie = matches!(c.kind, ChartType::Pie | ChartType::Doughnut);
    // Legend placed by `c:legendPos`.
    let waterfall = c.kind == ChartType::Waterfall;
    // Each legend entry: its name, its key's paint and the key's outline.
    let legend_items = build_legend_items(rctx, c, pie, waterfall);
    area =
        draw_legend(ctx, &LegendDraw { rctx, chart: c, transform: m, items: &legend_items, base, text_color: text_col, width: w, height: h }, area);
    if pie {
        draw_pie(ctx, rctx, c, m, area, base, text_col, (w, h));
        return;
    }
    if matches!(c.kind, ChartType::Funnel | ChartType::Treemap | ChartType::Sunburst | ChartType::RegionMap) {
        crate::chartex::draw(ctx, rctx, c, m, area, base, text_col);
        return;
    }
    if matches!(c.kind, ChartType::Radar | ChartType::FilledRadar) {
        crate::radar::draw(ctx, rctx, c, m, area, base, text_col);
        return;
    }
    if let Some(layout) = c.format.plot_layout.as_ref().filter(|l| !l.inner) {
        area = layout.resolve(area, w, h);
    }
    let combo = c.kind == ChartType::Combo;
    // Bars are drawn first and lines over them; a combo chart has both.
    let bar_kind = c.series.iter().map(|s| c.series_kind(s)).find(|k| k.is_bar_like());
    let horiz = if combo { bar_kind.is_some_and(ChartType::horizontal) } else { c.kind.horizontal() };
    area = draw_axis_titles(ctx, rctx, &AxisTitleDrawing { chart: c, transform: m, base, text_color: text_col, horiz, width: w, height: h }, area);
    // Once an axis has its own font size, the chart default no longer changes
    // either its text or the space and tick density reserved for that text.
    let value_size = axis_font_size(c, ChartTextRole::ValueAxis, base);
    let category_size = axis_font_size(c, ChartTextRole::CategoryAxis, base);
    let secondary_size = axis_font_size(c, ChartTextRole::SecondaryValueAxis, base);
    // As many major units as the value axis has room for labels (PowerPoint's automatic unit).
    let axis_len = if horiz { area.width() - category_size * 4.5 } else { area.height() - category_size * 1.8 };
    let max_intervals = (axis_len / if horiz { value_size * 6.0 } else { value_size * 1.9 }).floor().clamp(2.0, 12.0) as usize;
    // A logarithmic axis is scaled in exponents of its base.
    let log = c.log_axis_scale(max_intervals).map(|sc| (c.y_log_base.unwrap_or(10.0), sc));
    let final_scale = match log {
        Some((_, sc)) => sc,
        None => primary_scale(c, max_intervals),
    };
    // A manual plot first fits the labels at the initial aligned range. Only
    // contact with the value reserve triggers another unit normalization;
    // the opposite edge stays where that first layout placed it.
    let stage = log.is_none().then(|| column_axis_stage(rctx, c, area, (w, h), base, final_scale)).flatten();
    let (min, max, step) = stage.as_ref().map_or(final_scale, |stage| stage.scale);
    let initial_right = stage.and_then(|stage| stage.initial_right);
    let to_e = |v: f64| match log {
        Some((b, _)) => v.max(f64::MIN_POSITIVE).log(b).max(min - 1.0),
        None => v,
    };
    // A combo chart's secondary value axis, on the right (vertical value axes only).
    let second = if horiz { None } else { c.axis_scale(true, max_intervals) };
    let ticks = axis_ticks;
    // Label gutters as wide as the longest value label.
    let primary_ticks: Vec<f64> = ticks((min, max, step)).map(|k| log.map_or(k, |(b, _)| b.powf(k))).collect();
    let label_w = |sec: bool, sc: (f64, f64, f64)| {
        let vals: Vec<f64> = if sec { ticks(sc).collect() } else { primary_ticks.clone() };
        let size = if sec { secondary_size } else { value_size };
        vals.iter()
            .map(|v| {
                label_width(
                    rctx,
                    &c.axis_label(sec, *v),
                    size,
                    &c.text_props(if sec { ChartTextRole::SecondaryValueAxis } else { ChartTextRole::ValueAxis }),
                )
            })
            .fold(0.0_f64, f64::max)
    };
    let gutter = if horiz {
        let measured =
            c.categories.iter().map(|n| label_width(rctx, n, category_size, &c.text_props(ChartTextRole::CategoryAxis))).fold(0.0_f64, f64::max)
                + category_size * 0.6;
        clamp_soft(measured, category_size * 1.5, area.width() * 0.4)
    } else if c.y_hidden || c.y_tick_labels_hidden {
        value_size * 0.5
    } else {
        clamp_soft(label_w(false, (min, max, step)) + value_size * 0.6, value_size * 1.5, area.width() * 0.3)
    };
    let gutter_r = second
        .filter(|_| !c.y2_hidden && !c.y2_tick_labels_hidden)
        .map(|sc| clamp_soft(label_w(true, sc) + secondary_size * 0.6, secondary_size * 1.5, area.width() * 0.3))
        .unwrap_or(0.0);
    let (category_extra, category_offset) = category_layout_adjustment(rctx, c, base, area.width() - gutter - gutter_r, horiz);
    let plot = if horiz {
        Rect::new(area.x0 + gutter, area.y0, area.x1, area.y1 - value_size * 1.6)
    } else {
        Rect::new(area.x0 + gutter, area.y0, area.x1 - gutter_r, (area.y1 - category_size * 1.8 - category_extra).max(area.y0))
    };
    let reserve_width =
        if log.is_none() && c.kind == ChartType::Column { linear_axis_reserve_width(rctx, c, (min, max, step), value_size) } else { None };
    let measured = column_axis_plot(
        rctx,
        c,
        area,
        (w, h),
        base,
        reserve_width.unwrap_or_else(|| label_w(false, (min, max, step))),
        log.map_or(min, |(b, _)| b.powf(min)),
    )
    .map(|mut plot| {
        plot.rect.x1 = initial_right.map_or(plot.rect.x1, |right| right.max(plot.rect.x0));
        plot
    });
    let (plot, category_offset) = measured
        .as_ref()
        .map_or_else(|| (hidden_axis_plot(c, area, w, h).unwrap_or(plot), category_offset), |plot| (plot.rect, plot.category_offset));
    let plot = c
        .format
        .plot_layout
        .as_ref()
        .filter(|l| l.inner)
        .map_or(plot, |l| if measured.is_some() && authored_inner_plot(c).is_some() { plot } else { l.resolve(plot, w, h) });
    if let Some(f) = &c.plot_fill {
        crate::paint::fill_path(ctx, rctx, f, None, &plot.to_path(0.1), plot, m, deckcraft_geom::preset::FillMode::Norm, 1.0, rctx.pres);
    }
    let span = positive_span(max - min);
    // Values past the axis limits (fixed ones, or zero on an axis that starts above it) stop at
    // the plot's edge.
    let to_v = |v: f64| {
        let v = to_e(v).max(min).min(max);
        if horiz { plot.x0 + (v - min) / span * plot.width() } else { plot.y1 - (v - min) / span * plot.height() }
    };
    // A series' value as a position on its own axis.
    let to_axis = |sec: bool, v: f64| match (sec, second) {
        (true, Some((min2, max2, _))) => plot.y1 - (v.max(min2).min(max2) - min2) / positive_span(max2 - min2) * plot.height(),
        _ => to_v(v),
    };
    let grid_draw = GridlineDrawing {
        rctx,
        chart: c,
        transform: m,
        area,
        plot,
        horiz,
        base,
        text_color: text_col,
        grid_color: grid_col,
        min,
        max,
        step,
        log,
        primary_ticks: &primary_ticks,
        second,
        to_v: &to_v,
        to_axis: &to_axis,
        axis_ticks: axis_layers,
    };
    draw_gridlines(ctx, &grid_draw, !axis_layers);
    let ncat = c.categories.len().max(c.series.iter().map(|s| s.values.len()).max().unwrap_or(0)).max(1);
    // Forecast reserves empty category slots so the line can run past the data. Charts with no
    // forecast keep the old band.
    let (back, fwd) = forecast_pad(c);
    let nspan = ncat as f64 + back + fwd;
    let band = if horiz { plot.height() / nspan } else { plot.width() / nspan };
    // Where category `i`'s band starts along its axis: left to right, or bottom to top on a
    // horizontal bar chart, unless the axis is reversed. The forecast's empty slots stay on
    // their own side of the data.
    let backwards = horiz != c.cat_reversed;
    let slot = |i: usize| if backwards { ncat.saturating_sub(i + 1) as f64 + fwd } else { i as f64 + back };
    // Scatter and bubble charts have a value x axis: one scale over every series' x values, with its
    // own tick labels in place of category names.
    let xscale = if horiz { None } else { value_x_scale(c, (plot.width() / (category_size * 2.5)).floor().clamp(2.0, 12.0) as usize) };
    let to_x = |v: f64| xscale.map(|(lo, hi, _)| plot.x0 + (v - lo) / positive_span(hi - lo) * plot.width());
    let zero = match log {
        Some((b, _)) => to_v(b.powf(min)),
        None => to_v(clamp_soft(0.0, min, max)),
    };
    let (crossing, tick_axis, tick_high) = category_axis_placement(c, plot, zero, &to_v, axis_layers);
    let category_draw = CategoryLabelDrawing {
        rctx,
        chart: c,
        transform: m,
        area,
        plot,
        horiz,
        band,
        slot: &slot,
        xscale,
        base,
        text_color: text_col,
        offset: category_offset,
        axis: tick_axis,
        high: tick_high,
        measured_gap: axis_layers,
        view,
        frame_height: h,
    };
    if !axis_layers {
        draw_category_labels(ctx, &category_draw);
    }
    if let Some(b) = boxes {
        crate::chartex::draw_boxes(ctx, rctx, c, b, m, plot, band, horiz, &to_v);
        return;
    }
    // Error bars, drawn over every series: (position across the value axis, from, to, cap length, cap).
    let mut bars = BarDrawing {
        frame: (w, h),
        rctx,
        chart: c,
        transform: m,
        plot,
        band,
        horiz,
        ncat,
        bar_kind,
        waterfall,
        slot: &slot,
        to_v: &to_v,
        to_axis: &to_axis,
        base,
        text_color: text_col,
        labels: if axis_layers { BarLabelPass::Deferred } else { BarLabelPass::Inline },
    };
    let mut err_marks = draw_bars(ctx, &bars);
    // Column marks lie below axis strokes; line marks lie above them. Point-label paint
    // is deferred until both mark passes finish, then tick glyphs form the final layer.
    if axis_layers {
        draw_axes(ctx, rctx, c, m, plot, horiz, crossing);
        // Rewalk bar geometry to paint each label immediately, without retaining
        // one record per mark. Supported Column/Line charts have only one family;
        // mixed charts retain their existing inline pass until independently proven.
        bars.labels = BarLabelPass::Only;
        draw_bars(ctx, &bars);
    }
    // A stock chart's high-low lines and up/down bars (or close dashes) stand in for its series.
    if c.kind == ChartType::Stock {
        for i in 0..ncat {
            crate::radar::stock_marks(ctx, rctx, c, m, plot.x0 + band * (i as f64 + 0.5), band, i, &|v| to_axis(false, v));
        }
    }
    draw_lines(
        ctx,
        &mut LineDrawing {
            frame: (w, h),
            chart: c,
            rctx,
            transform: m,
            plot,
            category_count: ncat,
            band,
            slot: &slot,
            to_x: &to_x,
            to_axis: &to_axis,
            zero,
            base,
            error_marks: &mut err_marks,
            text_color: text_col,
        },
    );
    // A category position (0-based, fractional) as a slot, the way `slot` places the bars.
    let slot_at = |x: f64| if backwards { ncat as f64 - 1.0 - x + fwd } else { x + back };
    draw_trendlines(ctx, rctx, c, m, TrendlineLayout { plot, band, slot: &slot_at, xscale, ncat, horiz, base, text_col, to_axis: &to_axis });
    draw_error_marks(ctx, m, horiz, &err_marks, text_col);
    if axis_layers {
        // Tick text is clipped to the chart frame, independently of point-label
        // overflow and of the earlier series/grid/axis stroke passes.
        ctx.set_transform(m);
        ctx.push_clip_path(&Rect::new(0., 0., w, h).to_path(0.1));
        draw_value_labels(ctx, &grid_draw);
        draw_category_labels(ctx, &category_draw);
        ctx.pop_clip_path();
    } else {
        draw_axes(ctx, rctx, c, m, plot, horiz, crossing);
    }
}

fn draw_error_marks(ctx: &mut RenderContext, m: Affine, horiz: bool, marks: &[(f64, f64, f64, f64, bool)], text_col: Rgba) {
    if !marks.is_empty() {
        let mut p = BezPath::new();
        for &(mid, a, b, cap, end_cap) in marks {
            // A horizontal bar chart's error bars run across, its value axis being horizontal.
            let pt = |along: f64, across: f64| if horiz { Point::new(along, across) } else { Point::new(across, along) };
            p.move_to(pt(a, mid));
            p.line_to(pt(b, mid));
            if end_cap {
                for e in [a, b] {
                    p.move_to(pt(e, mid - cap));
                    p.line_to(pt(e, mid + cap));
                }
            }
        }
        ctx.set_transform(m);
        ctx.set_paint(color(text_col, 1.0));
        ctx.set_stroke(kurbo::Stroke::new(0.75));
        ctx.stroke_path(&p);
    }
}

fn draw_axes(ctx: &mut RenderContext, rctx: &Ctx, c: &Chart, m: Affine, plot: Rect, horiz: bool, zero: f64) {
    // Axis line.
    let mut ax = BezPath::new();
    if horiz {
        ax.move_to((zero, plot.y0));
        ax.line_to((zero, plot.y1));
    } else {
        ax.move_to((plot.x0, zero));
        ax.line_to((plot.x1, zero));
    }
    if !c.cat_deleted {
        chart_stroke(ctx, rctx, m, &ax, c.format.lines.get(&ChartLineRole::CategoryAxis), Rgba::rgb(0xBF, 0xBF, 0xBF));
    }
    for (role, secondary, hidden) in [(ChartLineRole::ValueAxis, false, c.y_hidden), (ChartLineRole::SecondaryValueAxis, true, c.y2_hidden)] {
        if !hidden && let Some(line) = c.format.lines.get(&role) {
            let points = if horiz {
                [(plot.x0, plot.y1), (plot.x1, plot.y1)]
            } else {
                let x = if secondary { plot.x1 } else { plot.x0 };
                [(x, plot.y0), (x, plot.y1)]
            };
            let path = polyline(&points.map(Point::from));
            chart_stroke(ctx, rctx, m, &path, Some(line), Rgba::rgb(0xBF, 0xBF, 0xBF));
        }
    }
}

struct LegendDraw<'a, 'ctx> {
    rctx: &'a Ctx<'ctx>,
    chart: &'a Chart,
    transform: Affine,
    items: &'a [(String, Paint, Option<(Rgba, f64)>)],
    base: f64,
    text_color: Rgba,
    width: f64,
    height: f64,
}

fn draw_legend(ctx: &mut RenderContext, draw: &LegendDraw<'_, '_>, mut area: Rect) -> Rect {
    let rctx = draw.rctx;
    let c = draw.chart;
    let m = draw.transform;
    let props = c.text_props(ChartTextRole::Legend);
    let base = props.size.unwrap_or(draw.base).clamp(1.0, 4000.0);
    let Some(pos) = c.legend.as_ref().filter(|_| !draw.items.is_empty()) else { return area };
    let vertical = matches!(pos.as_str(), "l" | "r" | "tr");
    let manual = c.format.layouts.get(&ChartTextRole::Legend);
    let max_width =
        manual.and_then(|l| l.w).filter(|v| *v > 0.0).map_or(draw.width * if vertical { 0.4 } else { 0.9 }, |v| v * draw.width).max(base * 2.0);
    let mut labels = Vec::new();
    let (mut x, mut y, mut row_height, mut used_width) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    for (name, _, _) in draw.items {
        let text_width = label_width(rctx, name, base, &props).min((max_width - base * 1.6).max(1.0));
        let mut body =
            label_body(name, LabelStyle { size: base, align: Align::Left, anchor: Anchor::Middle, col: draw.text_color, bold: false }, &props);
        body.body.wrap = Some(true);
        if let Some(explicit) = c.format.bodies.get(&ChartTextRole::Legend) {
            body.body = merged_chart_body(explicit, &body.body);
        }
        let shape = Shape { text: Some(body.clone()), ..Default::default() };
        let height = deckcraft_text::layout(
            rctx,
            &shape,
            &body,
            &deckcraft_text::Opts {
                rect: Rect::new(0.0, 0.0, text_width, 1_000_000.0),
                fields: &deckcraft_text::NoFields,
                prompt_color: None,
                no_shrink: true,
            },
        )
        .content_height
        .max(base * 1.3);
        let width = text_width + base * 1.6;
        if x > 0.0 && (vertical || x + width > max_width) {
            y += row_height;
            x = 0.0;
            row_height = 0.0;
        }
        labels.push((Point::new(x, y), text_width, height, body));
        row_height = row_height.max(height + base * 0.2);
        x += width;
        used_width = used_width.max(x);
    }
    let (origin, plot_area) = legend_layout(pos, (used_width, y + row_height), area, base, c.legend_overlay);
    area = plot_area;
    let natural = Rect::new(origin.x, origin.y, origin.x + used_width, origin.y + y + row_height);
    let bounds = manual.map_or(natural, |l| l.resolve(natural, draw.width, draw.height));
    let origin = Point::new(bounds.x0, bounds.y0);
    if !vertical && manual.is_some_and(|l| l.w.is_some()) {
        // A manually sized horizontal legend spreads each row within its box, including
        // equal leading/trailing gaps. Keep wrapped rows independent.
        for row in labels.chunk_by_mut(|a, b| (a.0.y - b.0.y).abs() < 0.01) {
            let used = row.last().map_or(0.0, |(offset, width, _, _)| offset.x + width + base * 1.6);
            let gap = (bounds.width() - used).max(0.0) / (row.len() + 1) as f64;
            for (i, (offset, _, _, _)) in row.iter_mut().enumerate() {
                offset.x += gap * (i + 1) as f64;
            }
        }
    }
    for ((_, paint, outline), (offset, tw, height, body)) in draw.items.iter().zip(labels) {
        let x = origin.x + offset.x;
        let y = origin.y + offset.y;
        let key = Rect::new(x, y + base * 0.4, x + base * 1.4, y + base * 0.9);
        fill_with(ctx, paint, key, m);
        if let Some((oc, ow)) = outline.filter(|o| o.1 > 0.0) {
            ctx.set_transform(m);
            ctx.set_paint(color(oc, 1.0));
            ctx.set_stroke(kurbo::Stroke::new(ow.min(base * 0.2)));
            ctx.stroke_path(&key.to_path(0.1));
        }
        let shape = Shape { text: Some(body.clone()), ..Default::default() };
        crate::draw_text(
            ctx,
            rctx,
            &shape,
            &body,
            m,
            &deckcraft_text::Opts {
                rect: Rect::new(x + base * 1.6, y, x + base * 1.6 + tw, y + height),
                fields: &deckcraft_text::NoFields,
                prompt_color: None,
                no_shrink: true,
            },
        );
    }
    area
}

fn build_legend_items(rctx: &Ctx, c: &Chart, pie: bool, waterfall: bool) -> Vec<(String, Paint, Option<(Rgba, f64)>)> {
    if waterfall {
        ["Increase", "Decrease", "Total"].iter().enumerate().map(|(i, n)| (n.to_string(), Paint::Color(series_color(rctx, c, i)), None)).collect()
    } else if matches!(c.kind, ChartType::Treemap | ChartType::Sunburst) {
        // A hierarchy's legend names its top-level groups.
        c.category_tree().iter().enumerate().map(|(i, n)| (n.name.clone(), Paint::Color(series_color(rctx, c, i)), None)).collect()
    } else if matches!(c.kind, ChartType::Funnel | ChartType::RegionMap) {
        // A map draws its own colour scale.
        vec![]
    } else if pie {
        c.categories.iter().enumerate().map(|(i, n)| (n.clone(), Paint::Color(series_color(rctx, c, i)), None)).collect()
    } else {
        let mut series: Vec<_> = c.series.iter().enumerate().collect();
        if c.kind == ChartType::Combo {
            series.sort_by_key(|(_, s)| s.secondary_axis);
        }
        series
            .into_iter()
            .map(|(i, s)| {
                let k = c.series_kind(s);
                let line = !k.is_bar_like() && !matches!(k, ChartType::Area | ChartType::StackedArea | ChartType::FilledRadar);
                if k == ChartType::Bubble {
                    (s.name.clone(), Paint::Marker(series_fill(rctx, c, i, Some(s))), None)
                } else if line {
                    (s.name.clone(), Paint::Line(series_line(rctx, c, i, s)), None)
                } else if k.is_bar_like() {
                    let paint = if c.vary_colors && c.series.len() == 1 && s.fill.is_none() {
                        Paint::Color(series_color(rctx, c, i))
                    } else {
                        paint_of(rctx, s.fill.as_ref(), series_color(rctx, c, i))
                    };
                    (s.name.clone(), paint, bar_outline(rctx, s))
                } else {
                    (s.name.clone(), Paint::Color(series_fill(rctx, c, i, Some(s))), None)
                }
            })
            .collect()
    }
}

struct GridlineDrawing<'a, 'ctx> {
    rctx: &'a Ctx<'ctx>,
    chart: &'a Chart,
    transform: Affine,
    area: Rect,
    plot: Rect,
    horiz: bool,
    base: f64,
    text_color: Rgba,
    grid_color: Rgba,
    min: f64,
    max: f64,
    step: f64,
    log: Option<(f64, (f64, f64, f64))>,
    primary_ticks: &'a [f64],
    second: Option<(f64, f64, f64)>,
    to_v: &'a dyn Fn(f64) -> f64,
    to_axis: &'a dyn Fn(bool, f64) -> f64,
    axis_ticks: bool,
}

fn draw_gridlines(ctx: &mut RenderContext, draw: &GridlineDrawing<'_, '_>, tick_text: bool) {
    let rctx = draw.rctx;
    let c = draw.chart;
    let m = draw.transform;
    let plot = draw.plot;
    let horiz = draw.horiz;
    let grid_col = draw.grid_color;
    let min = draw.min;
    let max = draw.max;
    let step = draw.step;
    let log = draw.log;
    let primary_ticks = draw.primary_ticks;
    let to_v = draw.to_v;
    // Minor gridlines, under the major ones: five to a major unit.
    if c.minor_gridlines {
        let minor = step / 5.0;
        let n = if minor > 0.0 { (((max - min) / minor).round() as i64).clamp(1, 500) } else { 1 };
        let mut g = BezPath::new();
        for k in (0..=n).filter(|k| k % 5 != 0) {
            // On a logarithmic axis `min`, `max` and `step` are exponents of the base.
            let e = min + k as f64 * minor;
            let pos = to_v(log.map_or(e, |(b, _)| b.powf(e)));
            if horiz {
                g.move_to((pos, plot.y0));
                g.line_to((pos, plot.y1));
            } else {
                g.move_to((plot.x0, pos));
                g.line_to((plot.x1, pos));
            }
        }
        chart_stroke(ctx, rctx, m, &g, c.format.lines.get(&ChartLineRole::MinorGrid), Rgba::rgb(0xF2, 0xF2, 0xF2));
    }
    // Legacy families retain their interleaved tick text. Layered Cartesian charts defer
    // the same glyph drawing until after series labels, without moving the grid strokes.
    let mut major = BezPath::new();
    let crossing = (draw.axis_ticks && log.is_none()).then(|| major_grid_crossing(c, min, max)).flatten();
    for &v in primary_ticks {
        let pos = to_v(v);
        if tick_text {
            draw_value_tick(ctx, draw, v, pos);
        }
        let mut g = BezPath::new();
        if horiz {
            g.move_to((pos, plot.y0));
            g.line_to((pos, plot.y1));
        } else {
            g.move_to((plot.x0, pos));
            g.line_to((plot.x1, pos));
        }
        if c.gridlines && crossing.is_none_or(|crossing| (v - crossing).abs() > step.abs() * 1e-9) {
            if tick_text {
                chart_stroke(ctx, rctx, m, &g, c.format.lines.get(&ChartLineRole::MajorGrid), grid_col);
            } else {
                major.extend(g.elements().iter().copied());
            }
        }
    }
    if c.gridlines && !tick_text {
        // One compound stroke preserves authored opacity and edge coverage
        // where grid lines overlap. The existing tick budget bounds this path.
        let line = c.format.lines.get(&ChartLineRole::MajorGrid);
        if !fill_overlapping_grid(ctx, rctx, m, &major, line, grid_col) {
            chart_stroke(ctx, rctx, m, &major, line, grid_col);
        }
    }
    if tick_text {
        draw_secondary_value_labels(ctx, draw);
    }
}

/// A painted category axis replaces the grid at its numeric crossing. An
/// authored value outside the scale does not suppress the clamped edge grid.
fn major_grid_crossing(c: &Chart, min: f64, max: f64) -> Option<f64> {
    if c.cat_deleted || c.format.lines.get(&ChartLineRole::CategoryAxis).is_some_and(|line| matches!(line.fill, Some(Fill::None))) {
        return None;
    }
    Some(match c.cat_crossing {
        Some(ChartAxisCrossing::Min) => min,
        Some(ChartAxisCrossing::Max) => max,
        Some(ChartAxisCrossing::Value(value)) if value.is_finite() => value,
        _ => 0_f64.max(min).min(max),
    })
}

fn draw_value_tick(ctx: &mut RenderContext, draw: &GridlineDrawing<'_, '_>, v: f64, pos: f64) {
    let c = draw.chart;
    let base = axis_font_size(c, ChartTextRole::ValueAxis, draw.base);
    let text = if c.y_hidden || c.y_tick_labels_hidden { String::new() } else { c.axis_label(false, v) };
    let (rect, align, anchor) = if draw.horiz {
        (Rect::new(pos - 30.0, draw.plot.y1 + 2.0, pos + 30.0, draw.plot.y1 + base * 1.5), Align::Center, Anchor::Top)
    } else {
        let gap = draw
            .axis_ticks
            .then(|| axis_metrics(draw.rctx, c, ChartTextRole::ValueAxis, base))
            .flatten()
            .map_or(base * 0.4, |metrics| metrics.height / 2. + metrics.ascent / 3.);
        let right = draw.plot.x0 - gap;
        // Automatic font-based plots can start left of the generic area inset.
        // Keep the tick's alignment edge even when no positive box width remains.
        let left = if draw.axis_ticks { draw.area.x0.min(right) } else { draw.area.x0 };
        (Rect::new(left, pos - base, right, pos + base), Align::Right, Anchor::Middle)
    };
    chart_label(
        ctx,
        draw.rctx,
        draw.transform,
        &text,
        rect,
        LabelStyle { size: base, align, anchor, col: draw.text_color, bold: false },
        (c, ChartTextRole::ValueAxis, draw.axis_ticks),
    );
}

fn draw_value_labels(ctx: &mut RenderContext, draw: &GridlineDrawing<'_, '_>) {
    for &v in draw.primary_ticks {
        draw_value_tick(ctx, draw, v, (draw.to_v)(v));
    }
    draw_secondary_value_labels(ctx, draw);
}

fn draw_secondary_value_labels(ctx: &mut RenderContext, draw: &GridlineDrawing<'_, '_>) {
    let c = draw.chart;
    // The secondary axis' labels, on the right, without gridlines of their own.
    if let Some(sc) = draw.second.filter(|_| !c.y2_hidden && !c.y2_tick_labels_hidden) {
        let base = axis_font_size(c, ChartTextRole::SecondaryValueAxis, draw.base);
        for v in axis_ticks(sc) {
            let pos = (draw.to_axis)(true, v);
            chart_label(
                ctx,
                draw.rctx,
                draw.transform,
                &c.axis_label(true, v),
                Rect::new(draw.plot.x1 + base * 0.4, pos - base, draw.area.x1, pos + base),
                LabelStyle { size: base, align: Align::Left, anchor: Anchor::Middle, col: draw.text_color, bold: false },
                (c, ChartTextRole::SecondaryValueAxis, draw.axis_ticks),
            );
        }
    }
}

/// Category-axis strokes and adjacent text share a crossing, while low/high text
/// remains attached to the plot. An explicit maximum crossing chooses the upper
/// side; a numeric crossing at the same coordinate still chooses the lower side.
fn category_axis_placement(c: &Chart, plot: Rect, zero: f64, to_v: &dyn Fn(f64) -> f64, enabled: bool) -> (f64, f64, bool) {
    if !enabled {
        return (zero, plot.y1, false);
    }
    let crossing = match c.cat_crossing {
        Some(ChartAxisCrossing::Min) => plot.y1,
        Some(ChartAxisCrossing::Max) => plot.y0,
        Some(ChartAxisCrossing::Value(v)) if v.is_finite() => to_v(v),
        _ => zero,
    };
    match c.cat_label_position {
        Some(ChartTickPosition::High) => (crossing, plot.y0, true),
        Some(ChartTickPosition::Low) => (crossing, plot.y1, false),
        _ => (crossing, crossing, c.cat_crossing == Some(ChartAxisCrossing::Max)),
    }
}

struct CategoryLabelDrawing<'a, 'ctx> {
    rctx: &'a Ctx<'ctx>,
    chart: &'a Chart,
    transform: Affine,
    area: Rect,
    plot: Rect,
    horiz: bool,
    band: f64,
    slot: &'a dyn Fn(usize) -> f64,
    xscale: Option<(f64, f64, f64)>,
    base: f64,
    text_color: Rgba,
    offset: f64,
    axis: f64,
    high: bool,
    measured_gap: bool,
    view: Affine,
    frame_height: f64,
}

fn draw_category_labels(ctx: &mut RenderContext, draw: &CategoryLabelDrawing<'_, '_>) {
    let rctx = draw.rctx;
    let c = draw.chart;
    if c.cat_hidden || c.cat_deleted {
        return;
    }
    let m = draw.transform;
    let area = draw.area;
    let plot = draw.plot;
    let horiz = draw.horiz;
    let band = draw.band;
    let slot = draw.slot;
    let xscale = draw.xscale;
    let base = c.text_props(ChartTextRole::CategoryAxis).size.unwrap_or(draw.base).clamp(1.0, 4000.0);
    let text_col = draw.text_color;
    let category_metrics = draw.measured_gap.then(|| axis_metrics(rctx, c, ChartTextRole::CategoryAxis, base)).flatten();
    let horizontal_category = !horiz
        && category_metrics.is_some()
        && c.format
            .bodies
            .get(&ChartTextRole::CategoryAxis)
            .is_none_or(|body| body.rot.unwrap_or(0.) == 0. && matches!(body.vert, None | Some(deckcraft_model::text::TextDir::Horizontal)));
    let gap = category_metrics
        .as_ref()
        .map_or(3., |metrics| if horizontal_category && !draw.high { 2. * metrics.ascent / 3. } else { 1.5 + metrics.height / 2. });
    let ticks = axis_ticks;
    if let Some(sc) = xscale {
        for v in ticks(sc) {
            let pos = plot.x0 + (v - sc.0) / positive_span(sc.1 - sc.0) * plot.width();
            chart_label(
                ctx,
                rctx,
                m,
                &fmt_num(v),
                Rect::new(pos - 30.0, plot.y1 + 3.0, pos + 30.0, plot.y1 + base * 1.6),
                LabelStyle { size: base, align: Align::Center, anchor: Anchor::Top, col: text_col, bold: false },
                (c, ChartTextRole::CategoryAxis, draw.measured_gap),
            );
        }
    }
    // Category labels: wrapped onto two lines when they don't fit their band (long date ranges).
    let wraps = !horiz && c.categories.iter().any(|n| label_width(rctx, n, base, &c.text_props(ChartTextRole::CategoryAxis)) > band);
    for (i, name) in c.categories.iter().enumerate().filter(|_| xscale.is_none()) {
        let mid = if horiz { plot.y0 + band * (slot(i) + 0.5) } else { plot.x0 + band * (slot(i) + 0.5) };
        if horiz {
            chart_label(
                ctx,
                rctx,
                m,
                name,
                Rect::new(area.x0, mid - base, plot.x0 - base * 0.4, mid + base),
                LabelStyle { size: base, align: Align::Right, anchor: Anchor::Middle, col: text_col, bold: false },
                (c, ChartTextRole::CategoryAxis, draw.measured_gap),
            );
        } else {
            let text = if wraps { split_label(name) } else { name.clone() };
            let extent = base * if wraps { 2.8 } else { 1.6 };
            let (top, bottom, anchor) = if draw.high {
                let height = category_metrics.as_ref().map_or(extent - 3., |metrics| {
                    if horizontal_category {
                        // A high-side frame includes each complete paragraph
                        // pitch. Exact points round before accumulation; default
                        // and percentage heights retain their continuous values.
                        let pitch = if matches!(c.line_spacing(ChartTextRole::CategoryAxis), Some(deckcraft_model::text::Spacing::Pts(_))) {
                            metrics.spaced_height.round()
                        } else {
                            metrics.spaced_height
                        };
                        let lines = text.split(['\r', '\n']).count().saturating_sub(text.matches("\r\n").count());
                        pitch * lines as f64
                    } else {
                        metrics.height * if wraps { 2. } else { 1. }
                    }
                });
                let top = (draw.axis - gap - draw.offset - height).max(0.);
                (top, top + height, Anchor::Top)
            } else {
                (draw.axis + gap + draw.offset, draw.axis + extent.max(gap) + draw.offset, Anchor::Top)
            };
            chart_label_with_metrics(
                ctx,
                rctx,
                m,
                &text,
                Rect::new(mid - band / 2.0, top, mid + band / 2.0, bottom),
                LabelStyle { size: base, align: Align::Center, anchor, col: text_col, bold: false },
                (
                    c,
                    ChartTextRole::CategoryAxis,
                    draw.measured_gap,
                    if !horizontal_category {
                        deckcraft_text::LineMetrics::Typographic
                    } else if draw.high {
                        deckcraft_text::LineMetrics::Chart
                    } else {
                        deckcraft_text::LineMetrics::ChartLow
                    },
                    horizontal_category.then_some(draw.view),
                    category_metrics.as_ref().filter(|_| horizontal_category && !draw.high).map(|metrics| {
                        let pitch = if matches!(c.line_spacing(ChartTextRole::CategoryAxis), Some(deckcraft_model::text::Spacing::Pts(_))) {
                            metrics.spaced_height.round()
                        } else {
                            metrics.spaced_height
                        };
                        (pitch, draw.frame_height)
                    }),
                ),
            );
        }
    }
}

struct AxisTitleDrawing<'a> {
    chart: &'a Chart,
    transform: Affine,
    base: f64,
    text_color: Rgba,
    horiz: bool,
    width: f64,
    height: f64,
}

fn draw_axis_titles(ctx: &mut RenderContext, rctx: &Ctx, draw: &AxisTitleDrawing<'_>, mut area: Rect) -> Rect {
    let c = draw.chart;
    let m = draw.transform;
    let base = draw.base;
    let text_col = draw.text_color;
    let horiz = draw.horiz;
    // Axis titles: the left one turned to read upwards, the bottom one centred under the axis.
    // On a horizontal bar chart the category axis is the left one.
    let (left_title, bottom_title) =
        if horiz { (c.axis_titles.0.as_deref(), c.axis_titles.1.as_deref()) } else { (c.axis_titles.1.as_deref(), c.axis_titles.0.as_deref()) };
    let bottom_role = if horiz { ChartTextRole::ValueTitle } else { ChartTextRole::CategoryTitle };
    let left_role = if horiz { ChartTextRole::CategoryTitle } else { ChartTextRole::ValueTitle };
    let bottom_size = c.text_props(bottom_role).size.unwrap_or(base).clamp(1.0, 4000.0);
    let left_size = c.text_props(left_role).size.unwrap_or(base).clamp(1.0, 4000.0);
    if let Some(t) = bottom_title.filter(|t| !t.is_empty()) {
        let band_h = bottom_size * 1.6;
        let automatic = Rect::new(area.x0, area.y1 - band_h, area.x1, area.y1);
        let r = c.format.layouts.get(&bottom_role).map_or(automatic, |l| l.resolve(automatic, draw.width, draw.height));
        chart_label(
            ctx,
            rctx,
            m,
            t,
            r,
            LabelStyle { size: base, align: Align::Center, anchor: Anchor::Bottom, col: text_col, bold: true },
            (c, bottom_role, false),
        );
        area.y1 -= band_h + base * 0.3;
    }
    if let Some(t) = left_title.filter(|t| !t.is_empty()) {
        let band_h = left_size * 1.6;
        let (cx, cy) = (area.x0 + band_h / 2.0, (area.y0 + area.y1) / 2.0);
        let explicit = c.format.bodies.get(&left_role).is_some_and(|p| p.rot.is_some() || p.vert.is_some());
        let automatic = Rect::new(area.x0, area.y0, area.x0 + band_h, area.y1);
        let placed = c.format.layouts.get(&left_role).map_or(automatic, |l| l.resolve(automatic, draw.width, draw.height));
        // Explicit body rotation already rotates the glyphs. Do not apply the default
        // quarter turn a second time. Manual positions are in chart coordinates.
        let (transform, rect) = if explicit {
            (m, placed)
        } else {
            let center = if c.format.layouts.contains_key(&left_role) { placed.center() } else { Point::new(cx, cy) };
            let turned = m * Affine::translate((center.x, center.y)) * Affine::rotate(-std::f64::consts::FRAC_PI_2);
            let half = placed.height() / 2.0;
            (turned, Rect::new(-half, -placed.width() / 2.0, half, placed.width() / 2.0))
        };
        chart_label(
            ctx,
            rctx,
            transform,
            t,
            rect,
            LabelStyle { size: base, align: Align::Center, anchor: Anchor::Middle, col: text_col, bold: true },
            (c, left_role, false),
        );
        area.x0 += band_h + base * 0.3;
    }
    area
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BarLabelPass {
    Inline,
    Deferred,
    Only,
}

struct BarDrawing<'a, 'ctx> {
    frame: (f64, f64),
    rctx: &'a Ctx<'ctx>,
    chart: &'a Chart,
    transform: Affine,
    plot: Rect,
    band: f64,
    horiz: bool,
    ncat: usize,
    bar_kind: Option<ChartType>,
    waterfall: bool,
    slot: &'a dyn Fn(usize) -> f64,
    to_v: &'a dyn Fn(f64) -> f64,
    to_axis: &'a dyn Fn(bool, f64) -> f64,
    base: f64,
    text_color: Rgba,
    labels: BarLabelPass,
}

struct LineDrawing<'a, 'ctx> {
    frame: (f64, f64),
    chart: &'a Chart,
    rctx: &'a Ctx<'ctx>,
    transform: Affine,
    plot: Rect,
    category_count: usize,
    band: f64,
    slot: &'a dyn Fn(usize) -> f64,
    to_x: &'a dyn Fn(f64) -> Option<f64>,
    to_axis: &'a dyn Fn(bool, f64) -> f64,
    zero: f64,
    base: f64,
    error_marks: &'a mut Vec<(f64, f64, f64, f64, bool)>,
    text_color: Rgba,
}

fn draw_lines(ctx: &mut RenderContext, draw: &mut LineDrawing<'_, '_>) {
    draw_bubbles(ctx, draw);
    let c = draw.chart;
    let rctx = draw.rctx;
    let m = draw.transform;
    let plot = draw.plot;
    let ncat = draw.category_count;
    let band = draw.band;
    let slot = draw.slot;
    let to_x = draw.to_x;
    let to_axis = draw.to_axis;
    let zero = draw.zero;
    let base = draw.base;
    let err_marks = &mut draw.error_marks;
    let mut acc = vec![0.0; ncat];
    let mut labels = vec![];
    for (si, s) in c.series.iter().enumerate() {
        let kind = c.series_kind(s);
        if kind.is_bar_like() || matches!(kind, ChartType::Stock | ChartType::Bubble) {
            continue;
        }
        let sec = c.on_secondary(s);
        let stacked = kind.stacked();
        let area_kind = matches!(kind, ChartType::Area | ChartType::StackedArea | ChartType::FilledRadar);
        let col = if area_kind { series_fill(rctx, c, si, Some(s)) } else { series_line(rctx, c, si, s) };
        let series_stats = s.error_bars.as_ref().map(|_| deckcraft_model::chart::SeriesStats::of(&s.values));
        let mut pts = vec![];
        // Whether the series has any x value, found once rather than per point.
        let any_x = s.x_values.iter().flatten().next().is_some();
        for i in 0..ncat {
            let Some(v) = s.values.get(i).copied().flatten() else { continue };
            let v = if stacked {
                let base_v = acc.get(i).copied().unwrap_or(0.0);
                if let Some(a) = acc.get_mut(i) {
                    *a += v;
                }
                base_v + v
            } else {
                v
            };
            let x = match kind {
                ChartType::Scatter | ChartType::Bubble => {
                    match (s.x_values.get(i).copied().flatten().and_then(to_x), any_x) {
                        (Some(x), _) => x,
                        // No x values at all: the points are numbered 1, 2, 3...
                        (None, false) => to_x(i as f64 + 1.0).unwrap_or(plot.x0 + band * (slot(i) + 0.5)),
                        (None, true) => continue,
                    }
                }
                _ => plot.x0 + band * (slot(i) + 0.5),
            };
            if let Some((lo, hi)) = s.error_bars.as_ref().and_then(|e| series_stats.as_ref().and_then(|st| e.range_in(st, i, v))) {
                err_marks.push((x, to_axis(sec, lo), to_axis(sec, hi), base * 0.4, s.error_bars.as_ref().is_some_and(|e| e.end_cap)));
            }
            pts.push(Point::new(x, to_axis(sec, v)));
            if let Some(t) = bar_data_label(c, s, i) {
                labels.push((Point::new(x, to_axis(sec, v)), t, si, i));
            }
        }
        if pts.is_empty() {
            continue;
        }
        let p = if s.smooth && !area_kind && pts.len() > 2 { smooth_path(&pts) } else { polyline(&pts) };
        ctx.set_transform(m);
        if area_kind {
            let mut a = p.clone();
            if let (Some(l), Some(f)) = (pts.last(), pts.first()) {
                a.line_to((l.x, zero));
                a.line_to((f.x, zero));
                a.close_path();
            }
            ctx.set_paint(color(col, 0.85));
            ctx.fill_path(&a);
        } else if kind != ChartType::Scatter {
            let lw = s.line.as_ref().and_then(|l| l.width).filter(|w| w.is_finite() && *w > 0.0).unwrap_or(2.25).clamp(0.25, 20.0);
            ctx.set_paint(color(col, 1.0));
            ctx.set_stroke(kurbo::Stroke::new(lw).with_join(kurbo::Join::Round).with_caps(kurbo::Cap::Round));
            ctx.stroke_path(&p);
        }
        if matches!(kind, ChartType::LineMarkers | ChartType::Scatter | ChartType::Bubble) {
            ctx.set_paint(color(col, 1.0));
            for q in &pts {
                ctx.fill_path(&kurbo::Circle::new(*q, 3.5).to_path(0.1));
            }
        }
    }
    // Later area series must not paint over earlier series' labels.
    for (q, text, si, i) in labels {
        let Some(series) = c.series.get(si) else { continue };
        point_data_label(
            ctx,
            rctx,
            m,
            &text,
            point_label_rect(q, base, plot),
            LabelStyle { size: base * 0.9, align: Align::Center, anchor: Anchor::Bottom, col: draw.text_color, bold: false },
            (c, series, i, Rect::new(q.x, q.y, q.x, q.y), draw.frame),
        );
    }
}

fn bubble_size(c: &Chart, s: &deckcraft_model::chart::Series, i: usize) -> Option<f64> {
    let size = if s.sizes.is_empty() { Some(1.0) } else { s.sizes.get(i).copied().flatten() };
    size.filter(|v| v.is_finite() && (*v > 0.0 || (c.bubbles.show_negative && *v < 0.0))).map(f64::abs)
}

fn bubble_point(c: &Chart, s: &deckcraft_model::chart::Series, i: usize) -> Option<(f64, f64, f64)> {
    let y = s.values.get(i).copied().flatten().filter(|v| v.is_finite())?;
    let x = if s.x_values.is_empty() { Some(i as f64 + 1.0) } else { s.x_values.get(i).copied().flatten().filter(|v| v.is_finite()) }?;
    Some((x, y, bubble_size(c, s, i)?))
}

/// Bubble area encodes the third data value; bubbles never draw a connecting polyline.
fn draw_bubbles(ctx: &mut RenderContext, draw: &LineDrawing<'_, '_>) {
    let c = draw.chart;
    let largest = c
        .series
        .iter()
        .filter(|s| c.series_kind(s) == ChartType::Bubble)
        .flat_map(|s| (0..s.values.len()).filter_map(move |i| bubble_point(c, s, i).map(|p| p.2)))
        .reduce(f64::max);
    let Some(largest) = largest else { return };
    let radius = draw.plot.width().min(draw.plot.height()).max(0.0) * 0.125 * f64::from(c.bubbles.scale.min(300)) / 100.0;
    for (si, s) in c.series.iter().enumerate().filter(|(_, s)| c.series_kind(s) == ChartType::Bubble) {
        for i in 0..s.values.len() {
            let Some((x, value, size)) = bubble_point(c, s, i) else { continue };
            let x = (draw.to_x)(x).unwrap_or(draw.plot.x0 + draw.band * ((draw.slot)(i) + 0.5));
            let center = Point::new(x, (draw.to_axis)(c.on_secondary(s), value));
            let relative = size / largest;
            let path = kurbo::Circle::new(center, radius * if c.bubbles.size_is_width { relative } else { relative.sqrt() }).to_path(0.1);
            let fill = s.point_fills.iter().find(|(point, _)| *point == i).map(|(_, f)| f).or(s.fill.as_ref());
            let fallback = series_fill(draw.rctx, c, si, Some(s));
            let fill = fill.cloned().unwrap_or_else(|| Fill::solid(ColorRef::rgb(fallback)));
            crate::paint::fill_path(
                ctx,
                draw.rctx,
                &fill,
                None,
                &path,
                path.bounding_box(),
                draw.transform,
                deckcraft_geom::preset::FillMode::Norm,
                1.0,
                draw.rctx.pres,
            );
            if let Some(line) = s.line.as_ref() {
                chart_stroke(ctx, draw.rctx, draw.transform, &path, Some(line), fallback);
            }
            if let Some(text) = bar_data_label(c, s, i) {
                point_data_label(
                    ctx,
                    draw.rctx,
                    draw.transform,
                    &text,
                    point_label_rect(center, draw.base, draw.plot),
                    LabelStyle { size: draw.base * 0.9, align: Align::Center, anchor: Anchor::Bottom, col: draw.text_color, bold: false },
                    (c, s, i, Rect::new(center.x, center.y, center.x, center.y), draw.frame),
                );
            }
        }
    }
}

fn point_label_rect(q: Point, base: f64, plot: Rect) -> Rect {
    let width = 80.0_f64.min(plot.width().max(0.0));
    let height = (base * 1.6).min(plot.height().max(0.0));
    let x = clamp_soft(q.x - width / 2.0, plot.x0, plot.x1 - width);
    let above = q.y - height - 4.0;
    let y = clamp_soft(if above >= plot.y0 { above } else { q.y + 4.0 }, plot.y0, plot.y1 - height);
    Rect::new(x, y, x + width, y + height)
}

fn draw_bars(ctx: &mut RenderContext, draw: &BarDrawing<'_, '_>) -> Vec<(f64, f64, f64, f64, bool)> {
    let c = draw.chart;
    let rctx = draw.rctx;
    let m = draw.transform;
    let plot = draw.plot;
    let band = draw.band;
    let horiz = draw.horiz;
    let ncat = draw.ncat;
    let bar_kind = draw.bar_kind;
    let waterfall = draw.waterfall;
    let slot = draw.slot;
    let to_v = draw.to_v;
    let to_axis = draw.to_axis;
    let base = draw.base;
    let mut err_marks: Vec<(f64, f64, f64, f64, bool)> = vec![];
    if let Some(bk) = bar_kind {
        let bars: Vec<(usize, &deckcraft_model::chart::Series)> =
            c.series.iter().enumerate().filter(|(_, s)| c.series_kind(s).is_bar_like()).collect();
        let ns = bars.len().max(1);
        // Gap width and overlap are fractions of one bar's width: a category's band holds its bars
        // (overlapping by `overlap` of a bar between neighbours) plus one gap.
        let gap = c.gap_width.clamp(0.0, 5.0);
        let stacked = bk.stacked();
        let ov = if stacked { 1.0 } else { c.overlap.clamp(-1.0, 1.0) };
        let n = ns as f64;
        let bar = band / (n - (n - 1.0) * ov + gap);
        let step = bar * (1.0 - ov);
        let group = bar + step * (n - 1.0);
        // Error bars need each series' mean and spread: worked out once, not per point.
        let stats: Vec<deckcraft_model::chart::SeriesStats> = bars.iter().map(|(_, s)| deckcraft_model::chart::SeriesStats::of(&s.values)).collect();
        // Each series' per-point fills by point (the first for a point wins), not searched per bar.
        let point_fills: Vec<HashMap<usize, &Fill>> = bars
            .iter()
            .map(|(_, s)| {
                let mut m = HashMap::new();
                for (k, f) in &s.point_fills {
                    m.entry(*k).or_insert(f);
                }
                m
            })
            .collect();
        // A waterfall's running bars, worked out once for all its categories.
        let waterfall_bars = if waterfall { c.waterfall_bars() } else { Vec::new() };
        for i in 0..ncat {
            let mut pos_acc = 0.0;
            let mut neg_acc = 0.0;
            let total: f64 = if bk.percent() {
                bars.iter().map(|(_, s)| s.values.get(i).copied().flatten().unwrap_or(0.0).abs()).sum::<f64>().max(1e-9)
            } else {
                1.0
            };
            for (bi, (si, s)) in bars.iter().enumerate() {
                let sec = c.on_secondary(s);
                let v = s.values.get(i).copied().flatten().unwrap_or(0.0) / total;
                let start = band * slot(i) + (band - group) / 2.0 + if stacked { 0.0 } else { step * bi as f64 };
                let (a, b) = if waterfall {
                    waterfall_bars.get(i).copied().unwrap_or_default()
                } else if stacked {
                    if v >= 0.0 {
                        let r = (pos_acc, pos_acc + v);
                        pos_acc += v;
                        r
                    } else {
                        let r = (neg_acc + v, neg_acc);
                        neg_acc += v;
                        r
                    }
                } else {
                    (0.0f64.min(v), 0.0f64.max(v))
                };
                let r = if horiz {
                    Rect::new(to_v(a), plot.y0 + start, to_v(b), plot.y0 + start + bar)
                } else {
                    Rect::new(plot.x0 + start, to_axis(sec, b), plot.x0 + start + bar, to_axis(sec, a))
                }
                .abs();
                if draw.labels == BarLabelPass::Only {
                    draw_bar_label(ctx, draw, s, i, r);
                    continue;
                }
                // A waterfall colours increases, decreases and totals, and joins each bar's end
                // to the next bar's start.
                let paint = if waterfall {
                    let slot = if c.subtotals.contains(&i) {
                        2
                    } else if b < a {
                        1
                    } else {
                        0
                    };
                    Paint::Color(series_color(rctx, c, slot))
                } else {
                    bar_paint(rctx, c, *si, i, s, point_fills.get(bi))
                };
                fill_with(ctx, &paint, r.abs(), m);
                if !waterfall && let Some((oc, ow)) = bar_outline(rctx, s).filter(|o| o.1 > 0.0) {
                    ctx.set_transform(m);
                    ctx.set_paint(color(oc, 1.0));
                    ctx.set_stroke(kurbo::Stroke::new(ow));
                    ctx.stroke_path(&r.abs().to_path(0.1));
                }
                if waterfall && s.extended_options(c).connector_lines && i + 1 < ncat && !horiz {
                    let y = to_v(b);
                    let mut j = BezPath::new();
                    j.move_to((r.x1, y));
                    j.line_to((r.x1 + (band - bar), y));
                    ctx.set_paint(color(Rgba::rgb(0xA6, 0xA6, 0xA6), 1.0));
                    ctx.set_stroke(kurbo::Stroke::new(0.75));
                    ctx.stroke_path(&j);
                }
                if let Some(e) = &s.error_bars {
                    let (end, mid) = if horiz { (if v >= 0.0 { b } else { a }, r.center().y) } else { (if v >= 0.0 { b } else { a }, r.center().x) };
                    if let Some((lo, hi)) = stats.get(bi).and_then(|st| e.range_in(st, i, end)) {
                        let (p0, p1) = if horiz { (to_v(lo), to_v(hi)) } else { (to_axis(sec, lo), to_axis(sec, hi)) };
                        err_marks.push((mid, p0, p1, bar.min(base * 0.8) * 0.5, e.end_cap));
                    }
                }
                if draw.labels == BarLabelPass::Inline {
                    draw_bar_label(ctx, draw, s, i, r);
                }
            }
        }
    }
    err_marks
}

fn draw_bar_label(ctx: &mut RenderContext, draw: &BarDrawing<'_, '_>, series: &deckcraft_model::chart::Series, point: usize, mark: Rect) {
    let Some(text) = bar_data_label(draw.chart, series, point) else { return };
    let rect = if draw.horiz {
        Rect::new(mark.x1 + 2., mark.y0, mark.x1 + 40., mark.y1)
    } else {
        Rect::new(mark.x0 - 10., mark.y0 - draw.base * 1.4, mark.x1 + 10., mark.y0)
    };
    point_data_label(
        ctx,
        draw.rctx,
        draw.transform,
        &text,
        rect,
        LabelStyle {
            size: draw.base * 0.9,
            align: if draw.horiz { Align::Left } else { Align::Center },
            anchor: if draw.horiz { Anchor::Middle } else { Anchor::Bottom },
            col: draw.text_color,
            bold: false,
        },
        (draw.chart, series, point, mark, draw.frame),
    );
}

fn bar_data_label(c: &Chart, s: &deckcraft_model::chart::Series, i: usize) -> Option<String> {
    c.point_label(s, i)
}

#[cfg(test)]
mod bar_label_tests {
    use super::bar_data_label;
    use deckcraft_model::chart::{Chart, Series};

    #[test]
    fn missing_bar_values_have_no_label_but_explicit_zero_does() {
        let chart = Chart { data_labels: true, ..Default::default() };
        let series = Series { values: [Some(1.0), None, Some(3.0)].into(), ..Default::default() };

        let labels: Vec<_> = (0..series.values.len()).filter_map(|i| bar_data_label(&chart, &series, i)).collect();

        assert_eq!(labels, ["1", "3"]);
        let zero = Series { values: [Some(0.0)].into(), ..Default::default() };
        assert_eq!(bar_data_label(&chart, &zero, 0).as_deref(), Some("0"));
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_pie(ctx: &mut RenderContext, rctx: &Ctx, c: &Chart, m: Affine, area: Rect, base: f64, text_col: Rgba, frame: (f64, f64)) {
    let Some(s) = c.series.first() else { return };
    let vals: Vec<f64> = s.values.iter().map(|v| v.unwrap_or(0.0).max(0.0)).collect();
    let total: f64 = vals.iter().sum();
    if total <= 0.0 {
        return;
    }
    let r = (area.width().min(area.height()) / 2.0) * 0.95;
    let cen = area.center();
    let mut a0 = -std::f64::consts::FRAC_PI_2;
    for (i, v) in vals.iter().enumerate() {
        let sweep = v / total * std::f64::consts::TAU;
        let mut p = BezPath::new();
        p.move_to(cen);
        let arc = kurbo::Arc::new(cen, (r, r), a0, sweep, 0.0);
        p.line_to(Point::new(cen.x + r * a0.cos(), cen.y + r * a0.sin()));
        p.extend(arc.append_iter(0.1));
        p.close_path();
        ctx.set_transform(m);
        let col = s
            .point_fills
            .iter()
            .find(|(k, _)| *k == i)
            .and_then(|(_, f)| if let Fill::Solid { color: cc } = f { Some(rctx.color(cc, None)) } else { None })
            .unwrap_or_else(|| series_color(rctx, c, i));
        ctx.set_paint(color(col, 1.0));
        ctx.fill_path(&p);
        ctx.set_paint(color(Rgba::WHITE, 1.0));
        ctx.set_stroke(kurbo::Stroke::new(1.0));
        ctx.stroke_path(&p);
        if let Some(text) = c.point_label(s, i) {
            let mid = a0 + sweep / 2.0;
            let lp = Point::new(cen.x + r * 0.65 * mid.cos(), cen.y + r * 0.65 * mid.sin());
            point_data_label(
                ctx,
                rctx,
                m,
                &text,
                Rect::new(lp.x - 30.0, lp.y - 10.0, lp.x + 30.0, lp.y + 10.0),
                LabelStyle {
                    size: base,
                    align: Align::Center,
                    anchor: Anchor::Middle,
                    // On the slices themselves: white, unless the chart's text was given a colour.
                    col: if c.text_color.is_some() { text_col } else { Rgba::WHITE },
                    bold: false,
                },
                (c, s, i, Rect::new(lp.x, lp.y, lp.x, lp.y), frame),
            );
        }
        a0 += sweep;
    }
    if c.kind == ChartType::Doughnut {
        ctx.set_transform(m);
        ctx.set_paint(color(Rgba::WHITE, 1.0));
        ctx.fill_path(&kurbo::Circle::new(cen, r * c.hole_size.clamp(0.1, 0.9)).to_path(0.1));
    }
}

/// `v` kept within `lo..=hi` without `f64::clamp`'s panic when `lo > hi` (a chart smaller than
/// its label font) or a bound is NaN: the upper bound wins.
pub(crate) fn clamp_soft(v: f64, lo: f64, hi: f64) -> f64 {
    let v = if v < lo { lo } else { v };
    if v > hi { hi } else { v }
}

/// A corner of `plot` whose box stays clear of `pts` (the drawn trendline).
fn caption_at(plot: Rect, box_w: f64, box_h: f64, pts: &[Point], n: usize) -> (f64, f64) {
    let pad = 4.0;
    let shift = n as f64 * (box_h + 2.0);
    let x_right = (plot.x1 - box_w - pad).max(plot.x0 + pad);
    let y_bottom = (plot.y1 - box_h - pad - shift).max(plot.y0 + pad);
    let y_top = (plot.y0 + pad + shift).min(y_bottom);
    let corners = [(plot.x0 + pad, y_top), (x_right, y_top), (plot.x0 + pad, y_bottom), (x_right, y_bottom)];
    let mut best = corners[0];
    let mut best_d = -1.0;
    for (x, y) in corners {
        let mut min_d = f64::MAX;
        for p in pts {
            let dx = if p.x < x {
                x - p.x
            } else if p.x > x + box_w {
                p.x - (x + box_w)
            } else {
                0.0
            };
            let dy = if p.y < y {
                y - p.y
            } else if p.y > y + box_h {
                p.y - (y + box_h)
            } else {
                0.0
            };
            min_d = min_d.min((dx * dx + dy * dy).sqrt());
        }
        if pts.is_empty() {
            min_d = 1.0;
        }
        if min_d > best_d {
            best_d = min_d;
            best = (x, y);
        }
    }
    best
}

fn pad_units(v: Option<f64>) -> f64 {
    v.filter(|v| v.is_finite() && *v > 0.0).map(|v| v.min(3.0)).unwrap_or(0.0)
}

/// Empty category slots reserved for a trendline forecast, capped so the data still fills the plot.
fn forecast_pad(c: &Chart) -> (f64, f64) {
    if matches!(c.kind, ChartType::Scatter | ChartType::Bubble) {
        return (0.0, 0.0);
    }
    let mut back = 0.0_f64;
    let mut fwd = 0.0_f64;
    for s in &c.series {
        if matches!(c.series_kind(s), ChartType::Scatter | ChartType::Bubble) {
            continue;
        }
        for t in s.trendlines.iter().take(8) {
            if t.kind == TrendlineType::MovingAvg {
                continue;
            }
            back = back.max(pad_units(t.backward));
            fwd = fwd.max(pad_units(t.forward));
        }
    }
    (back, fwd)
}

const TREND_INK: [Rgba; 4] = [Rgba::rgb(0x22, 0x22, 0x22), Rgba::rgb(0xB0, 0x30, 0x30), Rgba::rgb(0x1A, 0x5C, 0x7A), Rgba::rgb(0xA0, 0x68, 0x00)];

struct XYMapper<'a> {
    plot: Rect,
    band: f64,
    slot: &'a dyn Fn(f64) -> f64,
    horiz: bool,
    scatter: bool,
    scatter_x: (f64, f64),
    secondary: bool,
    to_axis: &'a dyn Fn(bool, f64) -> f64,
}

fn map_xy(mapper: &XYMapper<'_>, x: f64, y: f64) -> Option<Point> {
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let value = (mapper.to_axis)(mapper.secondary, y);
    if !value.is_finite() {
        return None;
    }
    let along = if mapper.scatter && mapper.scatter_x.1 > mapper.scatter_x.0 {
        mapper.plot.x0 + (x - mapper.scatter_x.0) / (mapper.scatter_x.1 - mapper.scatter_x.0) * mapper.plot.width()
    } else {
        let origin = if mapper.horiz { mapper.plot.y0 } else { mapper.plot.x0 };
        origin + mapper.band * ((mapper.slot)(x - 1.0) + 0.5)
    };
    if !along.is_finite() {
        return None;
    }
    Some(if mapper.horiz && !mapper.scatter { Point::new(value, along) } else { Point::new(along, value) })
}

/// The part of segment `a`–`b` that lies inside `r`.
fn clip_seg(a: Point, b: Point, r: Rect) -> Option<(Point, Point)> {
    let (mut t0, mut t1) = (0.0, 1.0);
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let p = [-dx, dx, -dy, dy];
    let q = [a.x - r.x0, r.x1 - a.x, a.y - r.y0, r.y1 - a.y];
    for i in 0..4 {
        let (pi, qi) = (p[i], q[i]);
        if pi.abs() < 1e-12 {
            if qi < 0.0 {
                return None;
            }
        } else {
            let t = qi / pi;
            if pi < 0.0 {
                if t > t1 {
                    return None;
                } else if t > t0 {
                    t0 = t;
                }
            } else if t < t0 {
                return None;
            } else if t < t1 {
                t1 = t;
            }
        }
    }
    Some((Point::new(a.x + t0 * dx, a.y + t0 * dy), Point::new(a.x + t1 * dx, a.y + t1 * dy)))
}

struct TrendlineLayout<'a> {
    plot: Rect,
    band: f64,
    slot: &'a dyn Fn(f64) -> f64,
    xscale: Option<(f64, f64, f64)>,
    ncat: usize,
    horiz: bool,
    base: f64,
    text_col: Rgba,
    to_axis: &'a dyn Fn(bool, f64) -> f64,
}

fn draw_trendlines(ctx: &mut RenderContext, rctx: &Ctx, c: &Chart, m: Affine, layout: TrendlineLayout<'_>) {
    let TrendlineLayout { plot, band, slot, xscale, ncat, horiz, base, text_col, to_axis } = layout;
    let mut label_n = 0usize;
    for s in &c.series {
        if s.trendlines.is_empty() {
            continue;
        }
        let kind = c.series_kind(s);
        let scatter = matches!(kind, ChartType::Scatter | ChartType::Bubble);
        let sec = c.on_secondary(s);
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for i in 0..ncat {
            let Some(y) = s.values.get(i).copied().flatten().filter(|v| v.is_finite()) else { continue };
            let x = if scatter {
                let Some(x) = s.x_values.get(i).copied().flatten().filter(|v| v.is_finite()) else { continue };
                x
            } else {
                (i + 1) as f64
            };
            xs.push(x);
            ys.push(y);
        }
        // The same x extent the scatter marks use, so the fit sits on the points: the chart's
        // value x axis where it has one, else the series' own x values.
        let (sx0, sx1) = if let (true, Some((lo, hi, _))) = (scatter, xscale) {
            (lo, hi)
        } else if scatter {
            let mut lo = f64::MAX;
            let mut hi = f64::MIN;
            for v in s.x_values.iter().copied().flatten().filter(|v| v.is_finite()) {
                lo = lo.min(v);
                hi = hi.max(v);
            }
            (lo, hi)
        } else {
            (0.0, 0.0)
        };
        let mapper = XYMapper { plot, band, slot, horiz, scatter, scatter_x: (sx0, sx1), secondary: sec, to_axis };
        for (ti, t) in s.trendlines.iter().take(8).enumerate() {
            let Some(fit) = t.fit(&xs, &ys) else { continue };
            let (x0, x1) = fit.span(t.forward, t.backward);
            let samples = fit.samples(x0, x1);
            let mut path = BezPath::new();
            let mut prev: Option<Point> = None;
            let mut ink_pts = Vec::new();
            let mut any = false;
            for w in samples.windows(2) {
                let (Some(a), Some(b)) = (map_xy(&mapper, w[0].0, w[0].1), map_xy(&mapper, w[1].0, w[1].1)) else {
                    prev = None;
                    continue;
                };
                let Some((ca, cb)) = clip_seg(a, b, plot) else {
                    prev = None;
                    continue;
                };
                let cont = prev.is_some_and(|p| (p.x - ca.x).abs() < 0.6 && (p.y - ca.y).abs() < 0.6);
                if cont {
                    path.line_to(cb);
                } else {
                    path.move_to(ca);
                    path.line_to(cb);
                }
                prev = Some(cb);
                if ink_pts.len() < 64 {
                    ink_pts.push(ca);
                    ink_pts.push(cb);
                }
                any = true;
            }
            if !any {
                continue;
            }
            ctx.set_transform(m);
            let ink = TREND_INK.get(ti % TREND_INK.len()).copied().unwrap_or(Rgba::rgb(0x22, 0x22, 0x22));
            ctx.set_paint(color(ink, 1.0));
            ctx.set_stroke(kurbo::Stroke::new(1.6).with_dashes(0.0, [6.0, 3.5]).with_caps(kurbo::Cap::Round));
            ctx.stroke_path(&path);
            let caption = t.caption(&fit);
            if caption.is_empty() {
                continue;
            }
            let lines = caption.chars().filter(|ch| *ch == '\n').count() + 1;
            let box_h = base * 1.3 * lines as f64 + 2.0;
            let widest = caption.lines().map(|l| l.chars().count()).max().unwrap_or(1) as f64;
            let box_w = clamp_soft(widest * base * 0.62, 48.0, plot.width() * 0.72);
            let (x, y) = caption_at(plot, box_w, box_h, &ink_pts, label_n);
            ctx.set_transform(m);
            ctx.set_paint(color(Rgba::WHITE, 1.0));
            ctx.fill_rect(&Rect::new(x, y, x + box_w, y + box_h));
            label(
                ctx,
                rctx,
                m,
                &caption,
                Rect::new(x, y, x + box_w, y + box_h),
                LabelStyle { size: (base * 0.9).clamp(8.0, 13.0), align: Align::Left, anchor: Anchor::Top, col: text_col, bold: false },
            );
            label_n += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect::new(0.0, 0.0, 400.0, 300.0);

    #[test]
    fn category_ellipsis_aligns_lines_but_centers_only_retained_literal_text() {
        use deckcraft_text::{LayoutMetrics, LineMetrics, NoFields, Opts};
        let p = deckcraft_model::Presentation::default();
        let ctx = Ctx::for_slide(&p, &p.slides[0]).unwrap();
        for (font, text, spacing, x) in [
            ("Inter", "A\nB\nC", 30., vec![402.2425, 393.9575]),
            ("Inter", "WW\nB\nC", 30., vec![385.295, 389.815]),
            ("Inter", "A\nWW\nC", 30., vec![406.745, 385.295]),
            ("Inter", "A\nB\nWW", 30., vec![402.2425, 393.9575]),
            ("Lato", "WW\nB\nC", 30., vec![384.29, 391.035]),
            ("Lato", "A\nWW\nC", 30., vec![405.73, 384.29]),
            ("Inter", "A\nB", 40., vec![398.10]),
        ] {
            let mut body = label_body(
                text,
                LabelStyle { size: 20., align: Align::Center, anchor: Anchor::Top, col: Rgba::BLACK, bold: false },
                &RunProps { font: Some(font.into()), ..Default::default() },
            );
            for para in &mut body.paragraphs {
                para.props.line_spacing = Some(deckcraft_model::text::Spacing::Pts(spacing));
            }
            let rect = Rect::new(90., 333., 720., 365.);
            let shape = Shape { text: Some(body.clone()), ..Default::default() };
            let layout = deckcraft_text::layout_with_metrics(
                &ctx,
                &shape,
                &body,
                &Opts { rect, fields: &NoFields, prompt_color: None, no_shrink: true },
                LayoutMetrics { line: LineMetrics::Chart, ..Default::default() },
            );
            let layout = category_tick_overflow(&ctx, &mut body, rect, layout, (spacing, 400.));
            assert_eq!(layout.lines.len(), x.len());
            for (i, expected) in x.into_iter().enumerate() {
                let actual = layout.runs.iter().find(|run| run.para == i).unwrap().glyphs[0].1;
                assert!((actual - expected).abs() < 0.02, "{font} {text:?}, line{i}: {actual} vs {expected}");
            }
        }
    }

    #[test]
    fn tiny_data_gets_several_ticks() {
        let sc = deckcraft_model::chart::axis_scale(1e-13, 5e-13, None, None, None, 8, true);
        assert!(axis_ticks(sc).count() >= 3, "{sc:?}");
    }

    #[test]
    fn dense_authored_ticks_reach_the_last_in_range_value() {
        for (lo, hi, step, count, last) in [
            (0., 49., 1., 50, 49.),
            (0., 50., 1., 51, 50.),
            (0., 51., 1., 52, 51.),
            (0., 499., 1., 500, 499.),
            (0., 500., 1., 501, 500.),
            (0., 501., 1.002, 501, 501.),
            (-501., 0., 1.002, 501, 0.),
            (-50., 51., 1., 102, 51.),
            (0., 50.1, 0.1002, 501, 50.1),
            (0., 2.5, 1., 3, 2.),
            (2.2, 4.7, 1., 3, 4.2),
        ] {
            let ticks: Vec<_> = axis_ticks((lo, hi, step)).collect();
            assert_eq!(ticks.len(), count, "{lo}..{hi} by{step}");
            assert!((ticks.last().copied().unwrap_or(f64::NAN) - last).abs() < 1e-9);
        }
    }

    #[test]
    fn malformed_tick_ranges_are_empty_and_dense_work_is_bounded() {
        for scale in [
            (0., 1., 0.),
            (0., 1., -1.),
            (0., 1., f64::NAN),
            (f64::NAN, 1., 1.),
            (0., f64::INFINITY, 1.),
            (0., 1., f64::INFINITY),
            (1., 0., 1.),
            (-f64::MAX, f64::MAX, 1.),
        ] {
            assert_eq!(axis_ticks(scale).count(), 0, "{scale:?}");
        }
        assert_eq!(axis_ticks((0., 1., 1e-30)).count(), 1001);
        assert_eq!(axis_ticks((2., 2., 1.)).collect::<Vec<_>>(), vec![2.]);
    }

    #[test]
    fn scatter_x_is_a_value_scale_over_all_series() {
        let mut c = Chart::sample(ChartType::Scatter);
        c.series[0].x_values = vec![Some(0.5), Some(7.2), None];
        assert_eq!(value_x_scale(&c, 8), Some((0.0, 8.0, 1.0)));
        assert_eq!(value_x_scale(&Chart::sample(ChartType::Column), 8), None);
    }

    #[test]
    fn bubble_axes_allow_glyph_room_but_keep_explicit_bounds() {
        let mut c = Chart::sample(ChartType::Bubble);
        let (lo, hi, _) = value_x_scale(&c, 8).unwrap();
        assert_eq!((lo, hi), (0.0, 5.0));
        let (lo, hi, _) = primary_scale(&c, 8);
        assert_eq!((lo, hi), (0.0, 6.0));
        c.y_min = Some(2.0);
        c.y_max = Some(5.0);
        c.y_major = Some(1.0);
        assert_eq!(primary_scale(&c, 8), (2.0, 5.0, 1.0));
        c.series[0].sizes = vec![Some(-2.0), Some(f64::NAN), Some(0.0), Some(4.0)];
        assert!(bubble_point(&c, &c.series[0], 0).is_none());
        assert!(bubble_point(&c, &c.series[0], 1).is_none());
        assert!(bubble_point(&c, &c.series[0], 2).is_none());
        assert_eq!(bubble_point(&c, &c.series[0], 3).unwrap().2, 4.0);
        c.bubbles.show_negative = true;
        assert_eq!(bubble_point(&c, &c.series[0], 0).unwrap().2, 2.0);
        c.series[0].x_values = vec![None];
        assert!(bubble_point(&c, &c.series[0], 0).is_none());
    }

    #[test]
    fn right_legend_is_a_centred_column_and_narrows_the_plot() {
        let (o, plot) = legend_layout("r", (60.0, 100.0), AREA, 10.0, false);
        assert_eq!((o.x, o.y), (340.0, 100.0));
        assert_eq!((plot.x0, plot.x1, plot.y0, plot.y1), (0.0, 335.0, 0.0, 300.0));
    }

    #[test]
    fn left_top_and_corner_positions() {
        let (o, plot) = legend_layout("l", (60.0, 100.0), AREA, 10.0, false);
        assert_eq!((o.x, o.y, plot.x0, plot.x1), (0.0, 100.0, 65.0, 400.0));
        let (o, plot) = legend_layout("t", (100.0, 14.0), AREA, 10.0, false);
        assert_eq!((o.x, o.y, plot.y0, plot.y1), (150.0, 0.0, 19.0, 300.0));
        let (o, plot) = legend_layout("tr", (60.0, 100.0), AREA, 10.0, false);
        assert_eq!((o.x, o.y, plot.x1), (340.0, 0.0, 335.0));
    }

    #[test]
    fn bottom_legend_keeps_the_old_row_layout() {
        let (o, plot) = legend_layout("b", (100.0, 14.0), AREA, 10.0, false);
        assert_eq!((o.x, o.y, plot.y1, plot.x1), (150.0, 286.0, 281.0, 400.0));
    }

    #[test]
    fn overlay_does_not_shrink_the_plot() {
        let (o, plot) = legend_layout("r", (60.0, 100.0), AREA, 10.0, true);
        assert_eq!(plot, AREA);
        assert_eq!(o.x, 340.0);
    }

    #[test]
    fn clip_keeps_the_part_inside_the_plot() {
        let r = Rect::new(0.0, 0.0, 100.0, 50.0);
        let (a, b) = clip_seg(Point::new(-50.0, 25.0), Point::new(150.0, 25.0), r).expect("clipped");
        assert!((a.x - 0.0).abs() < 1e-6 && (b.x - 100.0).abs() < 1e-6);
        assert!((a.y - 25.0).abs() < 1e-6);
        assert!(clip_seg(Point::new(-10.0, -10.0), Point::new(-1.0, -1.0), r).is_none());
        let (a, b) = clip_seg(Point::new(10.0, 10.0), Point::new(40.0, 20.0), r).expect("inside");
        assert!((a.x - 10.0).abs() < 1e-6 && (b.x - 40.0).abs() < 1e-6);
    }

    #[test]
    fn caption_keeps_clear_of_a_line_into_the_top_right() {
        let plot = Rect::new(0.0, 0.0, 200.0, 100.0);
        let pts = [Point::new(20.0, 80.0), Point::new(180.0, 15.0)];
        let (x, y) = caption_at(plot, 60.0, 20.0, &pts, 0);
        assert!(x > 100.0 && y > 40.0, "label on the line at {x} {y}");
    }

    #[test]
    fn forecast_reserves_at_most_three_category_slots() {
        use deckcraft_model::chart::Trendline;
        let mut c = Chart::sample(ChartType::Line);
        c.series[0].trendlines.push(Trendline { forward: Some(10.0), backward: Some(0.5), ..Default::default() });
        assert_eq!(forecast_pad(&c), (0.5, 3.0));
        c.series[0].trendlines[0].kind = TrendlineType::MovingAvg;
        assert_eq!(forecast_pad(&c), (0.0, 0.0));
        c.kind = ChartType::Scatter;
        c.series[0].trendlines[0].kind = TrendlineType::Linear;
        assert_eq!(forecast_pad(&c), (0.0, 0.0));
    }
}

#[cfg(test)]
mod typography_tests {
    use super::*;

    #[test]
    fn an_authored_plot_selects_the_axis_stage_from_its_value_reserve() {
        let p = deckcraft_model::Presentation::default();
        let ctx = Ctx::for_slide(&p, &p.slides[0]).unwrap();
        for (values, minimum, maximum, font, size, format, collision_x, clear_x, clear_count) in [
            ([73.2, 79.46], None, Some(81.3), "Inter", 20., "General", 107.31966, 109.31966, 716),
            ([-79.46, -73.2], Some(-81.3), None, "Lato", 20., "General", 112.43, 114.43, 716),
            ([73.2, 79.46], None, None, "Inter", 20., "General", 122.64193, 124.64193, 718),
            ([73.2, 79.46], None, Some(81.3), "Inter", 12., "0.0", 37.29297, 39.29297, 716),
        ] {
            let mut c = Chart::sample(ChartType::Column);
            c.title = None;
            c.legend = None;
            c.series.truncate(1);
            c.series[0].values = values.map(Some).to_vec();
            c.y_min = minimum;
            c.y_max = maximum;
            c.y_major = Some(0.0037);
            c.y_format = Some(format.into());
            c.text_styles.insert(ChartTextRole::ValueAxis, RunProps { font: Some(font.into()), size: Some(size), ..Default::default() });
            let final_scale = c.axis_scale(false, 5).unwrap();
            for (x, expected) in [(collision_x, 501), (clear_x, clear_count)] {
                c.format.plot_layout = Some(ChartLayout {
                    inner: true,
                    x: Some(x / 840.),
                    y: Some(0.15),
                    w: Some(0.7),
                    h: Some(0.6),
                    x_edge: true,
                    y_edge: true,
                    ..Default::default()
                });
                let stage = column_axis_stage(&ctx, &c, Rect::new(0., 0., 840., 420.), (840., 420.), 16., final_scale).unwrap();
                assert_eq!(axis_ticks(stage.scale).count(), expected, "{font}, {format}, x={x}");
                assert_eq!(stage.initial_right.is_some(), expected == 501);
            }
        }
    }

    #[test]
    fn category_measurement_has_a_work_budget_and_clamps_label_offset() {
        let p = deckcraft_model::Presentation::default();
        let ctx = Ctx::for_slide(&p, &p.slides[0]).unwrap();
        let mut c = Chart::sample(ChartType::Column);
        c.cat_label_offset = Some(u16::MAX);
        c.text_styles.insert(ChartTextRole::CategoryAxis, RunProps { font: Some("Inter".into()), size: Some(20.), ..Default::default() });
        for labels in [vec!["A\n".repeat(40_000)], vec!["A\nB".into(); 257]] {
            c.categories = labels;
            let (reserve, offset) = category_layout_adjustment(&ctx, &c, 20., 100., false);
            assert_eq!(reserve, offset, "over-budget caches skip the additional text pass");
            assert!(offset.is_finite() && (54. ..55.).contains(&offset), "{offset}");
        }
        c.cat_hidden = true;
        assert_eq!(category_layout_adjustment(&ctx, &c, 20., 100., false), (0., 0.));
    }

    #[test]
    fn point_labels_stay_inside_the_plot_at_its_edges() {
        let plot = Rect::new(0.0, 0.0, 200.0, 100.0);
        for point in [Point::new(0.0, 0.0), Point::new(200.0, 100.0), Point::new(100.0, 50.0)] {
            let rect = point_label_rect(point, 18.0, plot);
            assert!(rect.x0 >= plot.x0 && rect.x1 <= plot.x1 && rect.y0 >= plot.y0 && rect.y1 <= plot.y1);
        }
        assert!(point_label_rect(Point::new(100.0, 0.0), 18.0, plot).y0 > 0.0);
    }
    #[test]
    fn label_measurement_uses_proportional_glyphs_and_explicit_size() {
        let p = deckcraft_model::Presentation::default();
        let ctx = Ctx::for_slide(&p, &p.slides[0]).unwrap();
        let props = RunProps { font: Some("Inter".into()), size: Some(20.0), ..Default::default() };
        let narrow = label_width(&ctx, "IIII", 10.0, &props);
        let wide = label_width(&ctx, "WWWW", 10.0, &props);
        assert!(wide > narrow * 1.5, "equal character counts must not imply equal widths");
        let large = RunProps { size: Some(40.0), ..props.clone() };
        assert!((label_width(&ctx, "WWWW", 10.0, &large) / wide - 2.0).abs() < 0.01);
        let body = label_body("Example", LabelStyle { size: 10.0, align: Align::Left, anchor: Anchor::Top, col: Rgba::BLACK, bold: false }, &props);
        assert_eq!(body.paragraphs[0].runs[0].props.font.as_deref(), Some("Inter"));
        assert_eq!(body.paragraphs[0].runs[0].props.size, Some(20.0));
    }
}
