//! Tables: paint shared effective styles and immutable content geometry.
use crate::color;
use deckcraft_model::resolve::Ctx;
use deckcraft_model::table::{Cell, Table};
use deckcraft_model::text::TextDir;
use deckcraft_model::{Presentation, Shape};
use deckcraft_text::Fields;
use deckcraft_text::table::plain_row_heights;
use deckcraft_text::table::style_fill;
pub use deckcraft_text::table::{CellLook, block_body, cell_body, cell_rects, look, row_heights};
use kurbo::{Affine, BezPath, Rect, Shape as _};
use vello_cpu::{RenderContext, peniko};
/// Can a cell's text be left undrawn because it lands wholly outside the `target` (pixels)? Only
/// for cells whose text stays inside the cell: its row grew to fit it (one row, horizontal text,
/// not at the height cap), so nothing overflows. A margin covers italic overhang and the like.
/// A table of thousands of rows otherwise lays out every cell's text off the slide's bottom.
fn text_off_target(cell: &Cell, rect: Rect, m: Affine, target: (u16, u16)) -> bool {
    if cell.row_span > 1 || !matches!(cell.vertical, None | Some(TextDir::Horizontal)) || rect.height() >= 20_000.0 {
        return false;
    }
    let b = m.transform_rect_bbox(rect).inflate(64.0, 64.0);
    let view = Rect::new(0.0, 0.0, f64::from(target.0), f64::from(target.1));
    b.is_finite() && (b.x1 < view.x0 || b.y1 < view.y0 || b.x0 > view.x1 || b.y0 > view.y1)
}

#[cfg(test)]
thread_local! {
    /// Cells whose text `draw` laid out on this thread (tests check off-slide cells are skipped).
    pub(crate) static TEXT_CELLS_DRAWN: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Draws the table; returns the box its cells cover (table space), which grows with their text.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    ctx: &mut RenderContext,
    _pres: &Presentation,
    rctx: &Ctx,
    t: &Table,
    m: Affine,
    _w: f64,
    _h: f64,
    fields: &dyn deckcraft_text::Fields,
    editing: bool,
) -> Option<kurbo::Rect> {
    let plan = if t.rows.iter().flat_map(|row| &row.cells).any(|cell| !cell.blocks.is_empty()) {
        crate::table_content::layout(rctx, t, fields).ok()
    } else {
        None
    };
    draw_planned(ctx, rctx, t, m, fields, editing, plan.as_ref())
}

#[allow(clippy::too_many_arguments)]
fn draw_planned(
    ctx: &mut RenderContext,
    rctx: &Ctx,
    t: &Table,
    m: Affine,
    fields: &dyn Fields,
    editing: bool,
    plan: Option<&crate::table_content::TableLayout>,
) -> Option<Rect> {
    let rects = cell_rects(t, &plan.map(|p| p.heights.clone()).unwrap_or_else(|| plain_row_heights(rctx, t, fields)));
    let covered = rects.iter().map(|(_, _, r)| *r).reduce(|a, b| a.union(b));
    // Table backgrounds behind every cell: a file-defined style's, then the table's own fill.
    let style_bg = rctx
        .pres
        .table_styles
        .iter()
        .find(|d| d.id.eq_ignore_ascii_case(&t.style))
        .and_then(|d| d.background.as_ref())
        .and_then(|b| style_fill(rctx, b));
    let own_bg = t.background.clone().map(|f| (f, None));
    if let Some(all) = rects.iter().map(|(_, _, r)| *r).reduce(|a, b| a.union(b)) {
        for (bg, ph) in style_bg.into_iter().chain(own_bg) {
            ctx.set_transform(m);
            crate::paint::fill_path(ctx, rctx, &bg, ph, &all.to_path(0.1), all, m, deckcraft_geom::preset::FillMode::Norm, 1.0, rctx.pres);
        }
    }
    for (r, c, rect) in &rects {
        let Some(cell) = t.cell(*r, *c) else { continue };
        let lk = look(rctx, t, *r, *c);
        ctx.set_transform(m);
        match &cell.fill {
            Some(f) => {
                crate::paint::fill_path(ctx, rctx, f, None, &rect.to_path(0.1), *rect, m, deckcraft_geom::preset::FillMode::Norm, 1.0, rctx.pres)
            }
            None => match &lk.paint {
                Some((f, ph)) => {
                    crate::paint::fill_path(ctx, rctx, f, *ph, &rect.to_path(0.1), *rect, m, deckcraft_geom::preset::FillMode::Norm, 1.0, rctx.pres)
                }
                None => {
                    if let Some(fc) = lk.fill.filter(|f| f.a > 0) {
                        ctx.set_paint(color(fc, 1.0));
                        ctx.fill_rect(rect);
                    }
                }
            },
        }
    }
    // Diagonal borders, under the edges: drawn like any line (dashes, compound, caps).
    for (r, c, rect) in &rects {
        let Some(cell) = t.cell(*r, *c) else { continue };
        for (line, a, b) in [(&cell.diag_down, (rect.x0, rect.y0), (rect.x1, rect.y1)), (&cell.diag_up, (rect.x0, rect.y1), (rect.x1, rect.y0))] {
            let Some(l) = line.as_ref().filter(|l| l.fill.as_ref().is_some_and(|f| !f.is_none())) else { continue };
            let mut p = BezPath::new();
            p.move_to(a);
            p.line_to(b);
            crate::stroke(ctx, rctx, l, None, &p, None, m, l.width.unwrap_or(1.0).max(0.0), 1.0);
        }
    }
    // Borders: explicit cell borders, else the style's grid.
    for (r, c, rect) in &rects {
        let Some(cell) = t.cell(*r, *c) else { continue };
        let lk = look(rctx, t, *r, *c);
        let edges = [
            (rect.x0, rect.y0, rect.x0, rect.y1),
            (rect.x1, rect.y0, rect.x1, rect.y1),
            (rect.x0, rect.y0, rect.x1, rect.y0),
            (rect.x0, rect.y1, rect.x1, rect.y1),
        ];
        for (i, e) in edges.iter().enumerate() {
            let line = cell.borders.get(i).and_then(|b| b.as_ref());
            let (col, wd) = match line {
                // An explicit border is drawn like any line: dashed, compound, gradient.
                Some(l) => {
                    if l.fill.as_ref().is_some_and(|f| !f.is_none()) {
                        let mut p = BezPath::new();
                        p.move_to((e.0, e.1));
                        p.line_to((e.2, e.3));
                        crate::stroke(ctx, rctx, l, None, &p, None, m, l.width.unwrap_or(1.0).max(0.25), 1.0);
                    }
                    continue;
                }
                None => match lk.sides.get(i).copied().flatten().or(lk.border) {
                    Some(b) if b.0.a > 0 && b.1 > 0.0 => b,
                    _ => continue,
                },
            };
            let mut p = BezPath::new();
            p.move_to((e.0, e.1));
            p.line_to((e.2, e.3));
            ctx.set_transform(m);
            ctx.set_paint(color(col, 1.0));
            ctx.set_stroke(kurbo::Stroke::new(wd.max(0.25)));
            ctx.stroke_path(&p);
        }
    }
    // Ordered blocks use the same immutable plan as hit testing.
    if let Some(plan) = plan {
        for owner in &plan.cells {
            let Some(cell) = t.cell(owner.row, owner.col) else { continue };
            for block in &owner.blocks {
                match block {
                    crate::table_content::BlockLayout::Text { body, rect, .. } => {
                        if text_off_target(cell, *rect, m, (ctx.width(), ctx.height())) {
                            continue;
                        }
                        let shape = Shape { text: Some(body.clone()), ..Default::default() };
                        let mut layout = deckcraft_text::layout(
                            rctx,
                            &shape,
                            body,
                            &deckcraft_text::Opts { rect: *rect, fields, prompt_color: None, no_shrink: false },
                        );
                        if editing {
                            crate::preview_empty_bullets(&mut layout);
                        }
                        crate::draw_layout(ctx, &layout, *rect, m);
                    }
                    crate::table_content::BlockLayout::Table { id, transform, layout } => {
                        if let Some(child) = cell.blocks.iter().find_map(|b| match b {
                            deckcraft_model::table::CellBlock::Table(child) if child.id == *id => Some(child),
                            _ => None,
                        }) {
                            draw_planned(ctx, rctx, &child.table, m * *transform, fields, editing, Some(layout));
                        }
                    }
                }
            }
        }
        return covered;
    }
    // Text.
    for (r, c, rect) in &rects {
        let Some(cell) = t.cell(*r, *c) else { continue };
        if text_off_target(cell, *rect, m, (ctx.width(), ctx.height())) {
            continue;
        }
        let body = cell_body(rctx, t, *r, *c, cell);
        let tmp = Shape { text: Some(body.clone()), ..Default::default() };
        if body.is_empty() && !editing && !crate::has_paragraph_background(rctx, &tmp, &body) {
            continue;
        }
        #[cfg(test)]
        TEXT_CELLS_DRAWN.with(|n| n.set(n.get() + 1));
        let mut l = deckcraft_text::layout(rctx, &tmp, &body, &deckcraft_text::Opts { rect: *rect, fields, prompt_color: None, no_shrink: false });
        if editing {
            crate::preview_empty_bullets(&mut l);
        }
        crate::draw_layout(ctx, &l, *rect, m);
        let _ = peniko::Fill::NonZero;
    }
    covered
}
