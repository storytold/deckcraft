use super::*;

/// The presentation's drawing guides; the one being dragged at its new place, labelled with its
/// distance from the slide's centre in points.
pub(super) fn draw_guides(app: &SlideApp, painter: &egui::Painter, xf: Xf, slide: Rect, t: &Tokens) {
    let Some(st) = app.session.active() else { return };
    let size = st.doc.slide_size;
    let drag = app.session.tool.guide_drag;
    for (i, g) in st.doc.guides.iter().enumerate() {
        let (pos, active) = match drag {
            Some((di, p)) if di == i => (p, true),
            _ => (g.pos, app.session.tool.guide_hover == Some(i)),
        };
        let col = if active { t.accent } else { Color32::from_rgba_unmultiplied(120, 120, 120, 170) };
        let stroke = Stroke::new(1.0, col);
        let (a, b) = if g.vertical {
            let x = xf.to_screen(Point::new(pos, 0.0)).x;
            (pos2(x, slide.min.y), pos2(x, slide.max.y))
        } else {
            let y = xf.to_screen(Point::new(0.0, pos)).y;
            (pos2(slide.min.x, y), pos2(slide.max.x, y))
        };
        // Dashed, like PowerPoint's guides.
        let len = (b - a).length();
        let dir = (b - a) / len.max(1.0);
        let mut d = 0.0;
        while d < len {
            painter.line_segment([a + dir * d, a + dir * (d + 4.0).min(len)], stroke);
            d += 7.0;
        }
        if drag.is_some_and(|(di, _)| di == i) {
            let centre = if g.vertical { size.width / 2.0 } else { size.height / 2.0 };
            let off = pos - centre;
            let side = match (g.vertical, off < 0.0, off > 0.0) {
                (true, true, _) => " left of centre",
                (true, _, true) => " right of centre",
                (false, true, _) => " above centre",
                (false, _, true) => " below centre",
                _ => " (centre)",
            };
            let label = format!("{:.0} pt{side}", off.abs());
            let at = if g.vertical { pos2(a.x + 6.0, slide.min.y + 6.0) } else { pos2(slide.min.x + 6.0, a.y + 4.0) };
            let galley = painter.layout_no_wrap(label, theme::font(11.0), t.text);
            let r = Rect::from_min_size(at, galley.size()).expand(3.0);
            painter.rect_filled(r, CornerRadius::same(3), t.card);
            painter.galley(at, galley, t.text);
        }
    }
}

/// Screen-point distance between grid lines. `spacing` is the grid spacing in slide points;
/// `scale` maps slide points onto the screen. `None` when the grid would not be a finite set of
/// lines (bad inputs, or so fine that a line would land on every pixel).
pub(super) fn grid_line_step(spacing: f64, scale: f32) -> Option<f32> {
    if !spacing.is_finite() || !(1.0..=288.0).contains(&spacing) || !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let step = (spacing as f32) * scale;
    if !step.is_finite() || step < 1.0 {
        return None;
    }
    Some(step)
}

pub(super) fn draw_grid(painter: &egui::Painter, slide: Rect, step: f32) {
    let stroke = Stroke::new(0.5, Color32::from_black_alpha(40));
    // A denser grid than this paints a grey slab and costs a frame. Skip it.
    if !grid_fits(slide.width(), step) || !grid_fits(slide.height(), step) {
        return;
    }
    let mut x = slide.min.x + step;
    while x < slide.max.x {
        painter.line_segment([pos2(x, slide.min.y), pos2(x, slide.max.y)], stroke);
        x += step;
    }
    let mut y = slide.min.y + step;
    while y < slide.max.y {
        painter.line_segment([pos2(slide.min.x, y), pos2(slide.max.x, y)], stroke);
        y += step;
    }
}

fn grid_fits(span: f32, step: f32) -> bool {
    span.is_finite() && span > 0.0 && step >= 1.0 && span / step <= 800.0
}
