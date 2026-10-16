//! Effective table styles and geometry shared by renderers and interchange writers.
use crate::Fields;
use deckcraft_color::{ColorTransform, Rgba, SchemeSlot};
use deckcraft_geom::Rect;
use deckcraft_model::Shape;
use deckcraft_model::resolve::Ctx;
use deckcraft_model::style::{ColorRef, Fill};
use deckcraft_model::table::{Cell, Table, TableBorder, TablePart, TablePartStyle, TableStyleDef};
use deckcraft_model::text::{Anchor, TextBody, TextDir};
/// Style colours for a cell: (fill, text colour, bold).
pub struct CellLook {
    pub fill: Option<Rgba>,
    pub text: Option<Rgba>,
    pub bold: bool,
    pub border: Option<(Rgba, f64)>,
    /// Left, right, top and bottom borders from a file-defined style (a fully transparent colour
    /// means no border); `None` falls back to `border`.
    pub sides: [Option<(Rgba, f64)>; 4],
    /// A file-defined style's fill as a paint (gradients, see-through colours), with its
    /// placeholder colour; drawn instead of `fill`.
    pub paint: Option<(Fill, Option<Rgba>)>,
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

/// The look of cell (`r`, `c`) from a table style the file defines: each part that applies to
/// the cell (whole table, bands, first and last rows and columns, corners, in that order) sets the
/// fill, text and the borders on its region's edges (its inside lines within it).
fn look_def(ctx: &Ctx, def: &TableStyleDef, t: &Table, r: usize, c: usize) -> CellLook {
    let (rows, cols) = (t.rows.len(), t.cols.len());
    let (r1, c1) = t.cell(r, c).map(|cell| (r + cell.row_span.max(1) as usize - 1, c + cell.grid_span.max(1) as usize - 1)).unwrap_or((r, c));
    let (r1, c1) = (r1.min(rows.saturating_sub(1)), c1.min(cols.saturating_sub(1)));
    let last_r = rows.saturating_sub(1);
    let last_c = cols.saturating_sub(1);
    let header = t.first_row && r == 0;
    let total = t.last_row && r1 == last_r && rows > 1;
    let first_col = t.first_col && c == 0;
    let last_col = t.last_col && c1 == last_c && cols > 1;
    let body_r = r.saturating_sub(usize::from(t.first_row));
    let body_c = c.saturating_sub(usize::from(t.first_col));
    let mut look = CellLook { fill: None, text: None, bold: false, border: None, sides: [None; 4], paint: None };
    for part in TablePart::ALL {
        // (applies, region rows, region cols)
        let (on, region) = match part {
            TablePart::WholeTbl => (true, (0, last_r, 0, last_c)),
            TablePart::Band1H => (t.band_row && !header && !total && body_r % 2 == 0, (r, r1, 0, last_c)),
            TablePart::Band2H => (t.band_row && !header && !total && body_r % 2 == 1, (r, r1, 0, last_c)),
            TablePart::Band1V => (t.band_col && !first_col && !last_col && body_c % 2 == 0, (0, last_r, c, c1)),
            TablePart::Band2V => (t.band_col && !first_col && !last_col && body_c % 2 == 1, (0, last_r, c, c1)),
            TablePart::LastCol => (last_col, (0, last_r, c, last_c)),
            TablePart::FirstCol => (first_col, (0, last_r, 0, c1)),
            TablePart::LastRow => (total, (r, last_r, 0, last_c)),
            TablePart::FirstRow => (header, (0, r1, 0, last_c)),
            TablePart::SeCell => (total && last_col, (r, r1, c, c1)),
            TablePart::SwCell => (total && first_col, (r, r1, c, c1)),
            TablePart::NeCell => (header && last_col, (r, r1, c, c1)),
            TablePart::NwCell => (header && first_col, (r, r1, c, c1)),
        };
        if !on {
            continue;
        }
        let Some(ps) = def.part(part) else { continue };
        if let Some(p) = style_fill(ctx, ps) {
            look.fill = fill_color(ctx, &p.0, p.1);
            look.paint = Some(p);
        }
        if let Some(tc) = &ps.text_color {
            look.text = Some(ctx.color(tc, None));
        }
        if let Some(b) = ps.bold {
            look.bold = b;
        }
        let (r0, rr1, c0, cc1) = region;
        // Left, right, top, bottom of the cell: the part's edge or its inside line.
        let pick = [if c == c0 { 0 } else { 5 }, if c1 == cc1 { 1 } else { 5 }, if r == r0 { 2 } else { 4 }, if r1 == rr1 { 3 } else { 4 }];
        for (side, i) in pick.iter().enumerate() {
            if let (Some(Some(b)), Some(slot)) = (ps.borders.get(*i), look.sides.get_mut(side)) {
                *slot = Some(border_look(ctx, b));
            }
        }
    }
    look
}

/// A style part's fill, a theme fill reference resolved to the theme's fill with its colour as
/// the placeholder colour.
pub fn style_fill(ctx: &Ctx, ps: &TablePartStyle) -> Option<(Fill, Option<Rgba>)> {
    if let Some((idx, col)) = &ps.fill_ref {
        let ph = ctx.color(col, None);
        return Some(match ctx.master.theme.format.fills.get((*idx as usize).saturating_sub(1)) {
            Some(f) if *idx > 0 => (f.clone(), Some(ph)),
            _ if *idx == 0 => (Fill::None, None),
            _ => (Fill::solid(ColorRef::rgb(ph)), None),
        });
    }
    ps.fill.clone().map(|f| (f, None))
}

/// A fill's colour for a table cell (gradients and pictures by their first colour).
fn fill_color(ctx: &Ctx, f: &Fill, ph: Option<Rgba>) -> Option<Rgba> {
    match f {
        Fill::None => Some(Rgba::TRANSPARENT),
        Fill::Solid { color: c } => Some(ctx.color(c, ph)),
        Fill::Gradient(g) => g.stops.first().map(|s| ctx.color(&s.color, ph)),
        Fill::Pattern(p) => Some(ctx.color(&p.bg, ph)),
        _ => None,
    }
}

/// A style border's colour and width (a transparent colour for no line).
fn border_look(ctx: &Ctx, b: &TableBorder) -> (Rgba, f64) {
    match b {
        TableBorder::Line { line } => match &line.fill {
            Some(Fill::Solid { color: c }) => (ctx.color(c, None), line.width.unwrap_or(0.75)),
            Some(Fill::Gradient(g)) => (g.stops.first().map(|s| ctx.color(&s.color, None)).unwrap_or(Rgba::BLACK), line.width.unwrap_or(0.75)),
            _ => (Rgba::TRANSPARENT, 0.0),
        },
        TableBorder::Ref { idx, color } => {
            let w = ctx.master.theme.format.lines.get((*idx as usize).saturating_sub(1)).and_then(|l| l.width).unwrap_or(0.75);
            if *idx == 0 { (Rgba::TRANSPARENT, 0.0) } else { (ctx.color(color, None), w) }
        }
    }
}

pub fn look(ctx: &Ctx, t: &Table, r: usize, c: usize) -> CellLook {
    if let Some(def) = ctx.pres.table_styles.iter().find(|d| d.id.eq_ignore_ascii_case(&t.style)) {
        return look_def(ctx, def, t, r, c);
    }
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
        "none" => CellLook { fill: None, text: None, bold: false, border: None, sides: [None; 4], paint: None },
        "grid" => CellLook { fill: None, text: None, bold: false, border: Some((dark_text, 0.75)), sides: [None; 4], paint: None },
        "light1" => CellLook {
            fill: if band || band_c { Some(tint(20000)) } else { None },
            text: None,
            bold: header || total || first_col || last_col,
            border: if header { Some((a, 1.0)) } else { None },
            sides: [None; 4],
            paint: None,
        },
        "light2" => CellLook {
            fill: if header { Some(a) } else { None },
            text: if header { Some(white) } else { None },
            bold: header || total || first_col,
            border: Some((a, 1.0)),
            sides: [None; 4],
            paint: None,
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
            sides: [None; 4],
            paint: None,
        },
        "medium4" => CellLook {
            fill: Some(if header || band || band_c { tint(40000) } else { tint(20000) }),
            text: None,
            bold: header || total || first_col || last_col,
            border: Some((a, 0.75)),
            sides: [None; 4],
            paint: None,
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
            sides: [None; 4],
            paint: None,
        },
    }
}

/// Running edges 0, s0, s0+s1, … of a list of sizes (negative sizes count as 0).
pub(crate) fn edges(sizes: &[f64]) -> Vec<f64> {
    std::iter::once(0.0)
        .chain(sizes.iter().scan(0.0, |a, s| {
            *a += s.max(0.0);
            Some(*a)
        }))
        .collect()
}

/// A cell's text as drawn: its margins, anchor and direction, plus the table style's text colour and bold.
pub fn cell_body(rctx: &Ctx, t: &Table, r: usize, c: usize, cell: &Cell) -> TextBody {
    block_body(rctx, t, r, c, cell, &cell.text)
}

pub fn block_body(rctx: &Ctx, t: &Table, r: usize, c: usize, cell: &Cell, source: &TextBody) -> TextBody {
    let lk = look(rctx, t, r, c);
    let mut body = source.clone();
    let [ml, mr, mt, mb] = cell.margins.unwrap_or([7.2, 7.2, 3.6, 3.6]);
    body.body.inset_l = Some(ml);
    body.body.inset_r = Some(mr);
    body.body.inset_t = Some(mt);
    body.body.inset_b = Some(mb);
    body.body.anchor = Some(cell.anchor.unwrap_or(Anchor::Top));
    if cell.vertical.is_some() {
        body.body.vert = cell.vertical;
    }
    let formatted_color = cell.number_format.as_ref().and_then(|format| {
        let value = cell.data.as_ref().map(|data| data.value().clone()).or_else(|| {
            cell.formula.as_ref().and_then(|formula| match formula.outcome() {
                deckcraft_model::table::FormulaOutcome::Number { lexical } => {
                    Some(deckcraft_model::table::CellValue::Number { lexical: lexical.clone(), kind: deckcraft_model::table::CellNumberKind::Float })
                }
                deckcraft_model::table::FormulaOutcome::Boolean { value } => Some(deckcraft_model::table::CellValue::Boolean { value: *value }),
                deckcraft_model::table::FormulaOutcome::String { value } => Some(deckcraft_model::table::CellValue::String { value: value.clone() }),
                _ => None,
            })
        })?;
        format.format_value(&value).ok()?.color
    });
    for p in &mut body.paragraphs {
        for run in &mut p.runs {
            if let Some(color) = formatted_color {
                run.props.fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgba(color[0], color[1], color[2], color[3]))));
            }
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
    if t.rows.iter().flat_map(|row| &row.cells).any(|cell| !cell.blocks.is_empty()) {
        return crate::table_content::layout(rctx, t, fields)
            .map(|plan| plan.heights)
            .unwrap_or_else(|_| t.rows.iter().map(|row| row.height.max(0.0)).collect());
    }
    plain_row_heights(rctx, t, fields)
}

pub fn plain_row_heights(rctx: &Ctx, t: &Table, fields: &dyn Fields) -> Vec<f64> {
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
            let need = crate::fit_height(rctx, &tmp, &body, Rect::new(*x0, 0.0, *x1, h.max(1.0)), fields);
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
