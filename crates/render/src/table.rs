//! Tables: cell fills from the table style (or overrides), borders, and cell text.

use deckcraft_color::{ColorTransform, Rgba, SchemeSlot};
use deckcraft_model::resolve::Ctx;
use deckcraft_model::style::{ColorRef, Fill};
use deckcraft_model::table::{Cell, Table};
use deckcraft_model::text::{Anchor, TextBody, TextDir};
use deckcraft_model::{Presentation, Shape};
use deckcraft_text::Fields;
use kurbo::{Affine, BezPath, Rect, Shape as _};
use vello_cpu::{RenderContext, peniko};

use crate::color;

/// Style colours for a cell: (fill, text colour, bold).
pub struct CellLook {
    pub fill: Option<Rgba>,
    pub text: Option<Rgba>,
    pub bold: bool,
    pub border: Option<(Rgba, f64)>,
}

fn accent(ctx: &Ctx, style: &str) -> Rgba {
    let slot = match style.rsplit('-').next().unwrap_or("accent1") {
        "accent2" => SchemeSlot::Accent2,
        "accent3" => SchemeSlot::Accent3,
        "accent4" => SchemeSlot::Accent4,
        "accent5" => SchemeSlot::Accent5,
        "accent6" => SchemeSlot::Accent6,
        "tx1" => SchemeSlot::Tx1,
        _ => SchemeSlot::Accent1,
    };
    ctx.color(&ColorRef::scheme(slot), None)
}

pub fn look(ctx: &Ctx, t: &Table, r: usize, c: usize) -> CellLook {
    let a = accent(ctx, &t.style);
    let kind = t.style.split('-').next().unwrap_or("medium2");
    let header = t.first_row && r == 0;
    let total = t.last_row && r + 1 == t.rows.len();
    let first_col = t.first_col && c == 0;
    let last_col = t.last_col && c + 1 == t.cols.len();
    let body_r = if t.first_row { r.wrapping_sub(1) } else { r };
    let band = t.band_row && !header && !total && body_r % 2 == 0;
    let band_c = t.band_col && c.is_multiple_of(2);
    let tint = |k: i32| deckcraft_color::apply(a, &[ColorTransform::Tint(k)]);
    let white = Rgba::WHITE;
    let dark_text = ctx.color(&ColorRef::scheme(SchemeSlot::Tx1), None);
    match kind {
        "none" => CellLook { fill: None, text: None, bold: false, border: None },
        "grid" => CellLook { fill: None, text: None, bold: false, border: Some((dark_text, 0.75)) },
        "light1" => CellLook {
            fill: if band || band_c { Some(tint(20000)) } else { None },
            text: None,
            bold: header || total || first_col || last_col,
            border: if header { Some((a, 1.0)) } else { None },
        },
        "light2" => CellLook {
            fill: if header { Some(a) } else { None },
            text: if header { Some(white) } else { None },
            bold: header || total || first_col,
            border: Some((a, 1.0)),
        },
        "dark1" => CellLook {
            fill: Some(if header {
                Rgba::BLACK
            } else if band || band_c {
                deckcraft_color::apply(a, &[ColorTransform::Shade(60000)])
            } else {
                deckcraft_color::apply(a, &[ColorTransform::Shade(75000)])
            }),
            text: Some(white),
            bold: header || total || first_col,
            border: Some((white, 1.0)),
        },
        "medium4" => CellLook {
            fill: Some(if header || band || band_c { tint(40000) } else { tint(20000) }),
            text: None,
            bold: header || total || first_col || last_col,
            border: Some((a, 0.75)),
        },
        _ => CellLook {
            // medium2: strong header, banded light tints, white grid.
            fill: Some(if header || total || first_col || last_col {
                a
            } else if band || band_c {
                tint(40000)
            } else {
                tint(20000)
            }),
            text: if header || total || first_col || last_col { Some(white) } else { None },
            bold: header || total || first_col || last_col,
            border: Some((white, 1.0)),
        },
    }
}

/// Running edges 0, s0, s0+s1, … of a list of sizes (negative sizes count as 0).
fn edges(sizes: &[f64]) -> Vec<f64> {
    std::iter::once(0.0)
        .chain(sizes.iter().scan(0.0, |a, s| {
            *a += s.max(0.0);
            Some(*a)
        }))
        .collect()
}

/// A cell's text as drawn: its margins, anchor and direction, plus the table style's text colour and bold.
pub(crate) fn cell_body(rctx: &Ctx, t: &Table, r: usize, c: usize, cell: &Cell) -> TextBody {
    let lk = look(rctx, t, r, c);
    let mut body = cell.text.clone();
    let [ml, mr, mt, mb] = cell.margins.unwrap_or([7.2, 7.2, 3.6, 3.6]);
    body.body.inset_l = Some(ml);
    body.body.inset_r = Some(mr);
    body.body.inset_t = Some(mt);
    body.body.inset_b = Some(mb);
    body.body.anchor = Some(cell.anchor.unwrap_or(Anchor::Top));
    if cell.vertical.is_some() {
        body.body.vert = cell.vertical;
    }
    for p in &mut body.paragraphs {
        for run in &mut p.runs {
            if run.props.fill.is_none()
                && let Some(tc) = lk.text
            {
                run.props.fill = Some(Fill::solid(ColorRef::rgb(tc)));
            }
            if lk.bold && run.props.bold.is_none() {
                run.props.bold = Some(true);
            }
        }
    }
    body
}

/// Heights the rows are drawn at. A stored row height (`a:tr/@h`) is a minimum: the row grows so the
/// text of each of its cells fits, as in PowerPoint. Files from pandoc or python-pptx store 0.
/// Cells spanning several rows and vertical text keep the stored height.
pub fn row_heights(rctx: &Ctx, t: &Table, fields: &dyn Fields) -> Vec<f64> {
    let xs = edges(&t.cols);
    let mut out = Vec::with_capacity(t.rows.len());
    for (ri, row) in t.rows.iter().enumerate() {
        let mut h = row.height.max(0.0);
        for (ci, cell) in row.cells.iter().enumerate().take(t.cols.len()) {
            if cell.is_covered() || cell.row_span > 1 || !matches!(cell.vertical, None | Some(TextDir::Horizontal)) {
                continue;
            }
            let c1 = ci.saturating_add(cell.grid_span.max(1) as usize).min(t.cols.len());
            let (Some(x0), Some(x1)) = (xs.get(ci), xs.get(c1)) else { continue };
            let body = cell_body(rctx, t, ri, ci, cell);
            let tmp = Shape { text: Some(body.clone()), ..Default::default() };
            let need = deckcraft_text::fit_height(rctx, &tmp, &body, Rect::new(*x0, 0.0, *x1, h.max(1.0)), fields);
            if need.is_finite() {
                h = h.max(need.min(20_000.0));
            }
        }
        out.push(h);
    }
    out
}

/// Cell rectangles in table-local space, honouring merges: (row, col, rect) for visible cells.
/// `heights` are the drawn row heights ([`row_heights`]).
pub fn cell_rects(t: &Table, heights: &[f64]) -> Vec<(usize, usize, Rect)> {
    let mut out = vec![];
    let xs = edges(&t.cols);
    let ys = edges(heights);
    for (ri, row) in t.rows.iter().enumerate() {
        for (ci, cell) in row.cells.iter().enumerate().take(t.cols.len()) {
            if cell.is_covered() {
                continue;
            }
            let c1 = (ci + cell.grid_span.max(1) as usize).min(t.cols.len());
            let r1 = (ri + cell.row_span.max(1) as usize).min(t.rows.len());
            let (Some(x0), Some(x1), Some(y0), Some(y1)) = (xs.get(ci), xs.get(c1), ys.get(ri), ys.get(r1)) else { continue };
            out.push((ri, ci, Rect::new(*x0, *y0, *x1, *y1)));
        }
    }
    out
}

pub fn draw(ctx: &mut RenderContext, pres: &Presentation, rctx: &Ctx, t: &Table, m: Affine, _w: f64, _h: f64, fields: &dyn deckcraft_text::Fields) {
    let _ = pres;
    let rects = cell_rects(t, &row_heights(rctx, t, fields));
    for (r, c, rect) in &rects {
        let Some(cell) = t.cell(*r, *c) else { continue };
        let lk = look(rctx, t, *r, *c);
        ctx.set_transform(m);
        match &cell.fill {
            Some(f) => {
                crate::paint::fill_path(ctx, rctx, f, None, &rect.to_path(0.1), *rect, m, deckcraft_geom::preset::FillMode::Norm, 1.0, rctx.pres)
            }
            None => {
                if let Some(fc) = lk.fill {
                    ctx.set_paint(color(fc, 1.0));
                    ctx.fill_rect(rect);
                }
            }
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
                Some(l) => match &l.fill {
                    Some(Fill::Solid { color: cc }) => (rctx.color(cc, None), l.width.unwrap_or(1.0)),
                    Some(Fill::None) => continue,
                    _ => continue,
                },
                None => match lk.border {
                    Some(b) => b,
                    None => continue,
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
    // Text.
    for (r, c, rect) in &rects {
        let Some(cell) = t.cell(*r, *c) else { continue };
        if cell.text.is_empty() {
            continue;
        }
        let body = cell_body(rctx, t, *r, *c, cell);
        let tmp = Shape { text: Some(body.clone()), ..Default::default() };
        crate::draw_text(ctx, rctx, &tmp, &body, *rect, m, fields, None);
        let _ = peniko::Fill::NonZero;
    }
}
