//! Table Design and Table Layout tabs.

use deckcraft_geom::{Affine, Point, Rect};
use deckcraft_model::{Shape, ShapeKind, Table};
use serde_json::{Value, json};

use super::*;
use crate::{DocState, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!("table.insertRowAbove", "Insert Above", ["Table Layout", "Rows & Columns"], None, "{row?, id?}", has_selection, |s, p| rows_cols(
            s, p, "rowAbove"
        )),
        cmd!("table.insertRowBelow", "Insert Below", ["Table Layout", "Rows & Columns"], None, "{row?, id?}", has_selection, |s, p| rows_cols(
            s, p, "rowBelow"
        )),
        cmd!("table.insertColumnLeft", "Insert Left", ["Table Layout", "Rows & Columns"], None, "{col?, id?}", has_selection, |s, p| rows_cols(
            s, p, "colLeft"
        )),
        cmd!("table.insertColumnRight", "Insert Right", ["Table Layout", "Rows & Columns"], None, "{col?, id?}", has_selection, |s, p| rows_cols(
            s, p, "colRight"
        )),
        cmd!("table.deleteRow", "Delete Rows", ["Table Layout", "Rows & Columns"], None, "{row?, id?}", has_selection, |s, p| rows_cols(
            s, p, "delRow"
        )),
        cmd!("table.deleteColumn", "Delete Columns", ["Table Layout", "Rows & Columns"], None, "{col?, id?}", has_selection, |s, p| rows_cols(
            s, p, "delCol"
        )),
        cmd!("table.merge", "Merge Cells", ["Table Layout", "Merge"], None, "{from: [r, c], to: [r, c], id?}", has_selection, merge),
        cmd!(
            "table.split",
            "Split Cells",
            ["Table Layout", "Merge"],
            None,
            "{cell?: [r, c] (else the edited cell), id?} | {row?: index, col?: index, id?}",
            has_selection,
            split
        ),
        cmd!(
            "table.style",
            "Table Styles",
            ["Table Design", "Table Styles"],
            None,
            "{style: medium2-accent1|light1-accent2|dark1-tx1|grid|none…, id?}",
            has_selection,
            style
        ),
        cmd!(
            "table.options",
            "Table Style Options",
            ["Table Design", "Table Style Options"],
            None,
            "{headerRow?: bool, totalRow?: bool, bandedRows?: bool, firstColumn?: bool, lastColumn?: bool, bandedColumns?: bool, id?}",
            has_selection,
            options
        ),
        cmd!(
            "table.cellFill",
            "Shading",
            ["Table Design", "Table Styles"],
            None,
            "{color? | none?, cells?: [[r,c],…] (default: selected cells or all), id?}",
            has_selection,
            cell_fill
        ),
        cmd!(
            "table.borders",
            "Borders",
            ["Table Design", "Draw Borders"],
            None,
            "{which: all|outside|inside|insideH|insideV|top|bottom|left|right|diagDown|diagUp|none, color?, width?: pt, dash?: solid|dash|dot…, cells?: [[r,c],…] (default: selected cells or all), id?}",
            has_selection,
            borders
        ),
        cmd!(
            "table.cellMargins",
            "Cell Margins",
            ["Table Layout", "Alignment"],
            None,
            "{preset?: normal|none|narrow|wide, left?, right?, top?, bottom?: pt, cells?, id?}",
            has_selection,
            cell_margins
        ),
        cmd!(
            "table.cellAlign",
            "Align Cell Text",
            ["Table Layout", "Alignment"],
            None,
            "{anchor: top|middle|bottom, cells?, id?}",
            has_selection,
            cell_align
        ),
        cmd!("table.distributeRows", "Distribute Rows", ["Table Layout", "Cell Size"], None, "{id?}", has_selection, |s, p| distribute(s, p, true)),
        cmd!("table.distributeColumns", "Distribute Columns", ["Table Layout", "Cell Size"], None, "{id?}", has_selection, |s, p| distribute(
            s, p, false
        )),
        cmd!(
            "table.columnWidth",
            "Width",
            ["Table Layout", "Cell Size"],
            None,
            "{col, width: pt, keepTotal?: bool (the next column absorbs the change), id?}",
            has_selection,
            column_width
        ),
        cmd!("table.rowHeight", "Height", ["Table Layout", "Cell Size"], None, "{row, height: pt, id?}", has_selection, row_height),
        cmd!(
            "table.drawLine",
            "Draw Table",
            ["Table Design", "Draw Borders"],
            None,
            "{from: [x, y], to: [x, y] slide pt, id?: table (default: the one under `from`)} splits the cells the line crosses",
            has_slide,
            draw_line
        ),
        cmd!(
            "table.eraseLine",
            "Table Eraser",
            ["Table Design", "Draw Borders"],
            None,
            "{at: [x, y] slide pt, id?, tol?: pt (default 6)} merges the two cells on either side of the nearest border",
            has_slide,
            erase_line
        ),
        cmd!(noundo "table.selectCells", "Select Cells", [], None, "{from: [r,c], to: [r,c], id?}", has_selection, select_cells),
    ]
}

/// Narrowest a column gets when its border is dragged (points).
pub const MIN_COLUMN_WIDTH: f64 = 12.0;

/// A table as drawn on the slide: where it is, its drawn row heights and its visible cells.
#[derive(Clone, Debug)]
pub struct TableFrame {
    /// Table-local (points from the table's top-left corner) → slide space, as the table is drawn.
    pub affine: Affine,
    pub cols: Vec<f64>,
    /// Row heights as drawn: a stored height is a minimum, rows grow to fit their text.
    pub heights: Vec<f64>,
    /// Visible cells (merges honoured): (row, col, table-local rect).
    pub cells: Vec<(usize, usize, Rect)>,
}

/// A border between table rows or columns that can be dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableBorder {
    /// The right edge of this column (the last one is the table's right edge).
    Column(usize),
    /// The bottom edge of this row (the last one is the table's bottom edge).
    Row(usize),
}

/// The drawn geometry of table shape `sh` on the edited slide (`None` if it isn't a table).
pub fn table_frame(st: &DocState, sh: &Shape) -> Option<TableFrame> {
    let ShapeKind::Table(t) = &sh.kind else { return None };
    // The table is drawn in its own box; rows that grow past it don't move it.
    let x = sh.xfrm.unwrap_or_else(|| xfrm_of(&st.doc, &st.selection, sh));
    let heights = table_drawn_heights(&st.doc, &st.selection, t);
    let cells = deckcraft_render::table_cell_rects(t, &heights);
    Some(TableFrame { affine: x.affine(), cols: t.cols.clone(), heights, cells })
}

impl TableFrame {
    pub fn width(&self) -> f64 {
        self.cols.iter().map(|w| w.max(0.0)).sum()
    }

    pub fn height(&self) -> f64 {
        self.heights.iter().map(|h| h.max(0.0)).sum()
    }

    /// Table-local point for slide point `p`.
    pub fn local(&self, p: Point) -> Point {
        self.affine.inverse() * p
    }

    /// Table-local rect of visible cell (`r`, `c`).
    pub fn cell_rect(&self, r: usize, c: usize) -> Option<Rect> {
        self.cells.iter().find(|(cr, cc, _)| (*cr, *cc) == (r, c)).map(|x| x.2)
    }

    /// The visible cell at table-local point `p` (points outside the table go to the nearest cell).
    pub fn cell_at(&self, p: Point) -> Option<(usize, usize)> {
        let q = Point::new(p.x.clamp(0.0, self.width().max(0.0)), p.y.clamp(0.0, self.height().max(0.0)));
        let inside = |r: &Rect| q.x >= r.x0 && q.x <= r.x1 && q.y >= r.y0 && q.y <= r.y1;
        // Edges are shared: prefer the cell the point is strictly inside.
        self.cells
            .iter()
            .find(|(_, _, r)| q.x >= r.x0 && q.x < r.x1 && q.y >= r.y0 && q.y < r.y1)
            .or_else(|| self.cells.iter().rev().find(|(_, _, r)| inside(r)))
            .map(|(r, c, _)| (*r, *c))
    }

    /// The draggable border within `tol` of table-local point `p`: the inner borders and the right
    /// and bottom edges (the left and top edges move the table). Borders hidden inside merged
    /// cells don't count.
    pub fn border_at(&self, p: Point, tol: f64) -> Option<TableBorder> {
        let (w, h) = (self.width(), self.height());
        if !(p.x >= -tol && p.y >= -tol && p.x <= w + tol && p.y <= h + tol) {
            return None;
        }
        let q = Point::new(p.x.clamp(0.0, w), p.y.clamp(0.0, h));
        let mut best: Option<(f64, TableBorder)> = None;
        let mut x = 0.0;
        for (i, cw) in self.cols.iter().enumerate() {
            x += cw.max(0.0);
            let d = (p.x - x).abs();
            // A cell ending at this edge at this height shows the border.
            let shown = self.cells.iter().any(|(_, _, r)| (r.x1 - x).abs() < 1e-6 && q.y >= r.y0 && q.y <= r.y1);
            if d <= tol && shown && best.is_none_or(|b| d < b.0) {
                best = Some((d, TableBorder::Column(i)));
            }
        }
        let mut y = 0.0;
        for (i, rh) in self.heights.iter().enumerate() {
            y += rh.max(0.0);
            let d = (p.y - y).abs();
            let shown = self.cells.iter().any(|(_, _, r)| (r.y1 - y).abs() < 1e-6 && q.x >= r.x0 && q.x <= r.x1);
            if d <= tol && shown && best.is_none_or(|b| d < b.0) {
                best = Some((d, TableBorder::Row(i)));
            }
        }
        best.map(|b| b.1)
    }
}

fn cell_param(p: &Value, key: &str) -> Option<(usize, usize)> {
    let a = p.get(key)?.as_array()?;
    Some((a.first()?.as_u64()? as usize, a.get(1)?.as_u64()? as usize))
}

/// The table's row/col to act on: params, the edited cell, or the selected cells.
fn at(s: &Session, p: &Value) -> (usize, usize) {
    let st = s.active();
    let cell = st.and_then(|d| d.selection.text.as_ref().and_then(|t| t.cell)).or_else(|| st.and_then(|d| d.selection.cells.map(|c| c.1)));
    (usize_param(p, "row").or(cell.map(|c| c.0)).unwrap_or(0), usize_param(p, "col").or(cell.map(|c| c.1)).unwrap_or(0))
}

fn with_table(s: &mut Session, p: &Value, cmd: &str, f: impl Fn(&mut Table) -> Result<()>) -> Result<Value> {
    let id = match id_param(p, "id", cmd)? {
        Some(i) => i,
        None => {
            let st = s.doc()?;
            st.selection
                .text
                .as_ref()
                .filter(|t| t.cell.is_some())
                .map(|t| t.shape)
                .or(st.selection.cells.map(|c| c.0))
                .or_else(|| st.selection.shapes.first().copied())
                .ok_or_else(|| bad(cmd, "select a table"))?
        }
    };
    let r = edit_shapes(s, &json!({"id": id}), cmd, |sh| match &mut sh.kind {
        ShapeKind::Table(t) => {
            f(t)?;
            t.normalize();
            let (w, h) = (t.width(), t.height());
            if let Some(x) = sh.xfrm.as_mut() {
                x.w = w;
                x.h = h;
            }
            Ok(())
        }
        _ => Err(bad(cmd, "not a table")),
    })?;
    // The edited cell may have moved out of range.
    s.select(|doc, sel| {
        let dims = super::current_shapes(doc, sel)
            .and_then(|v| deckcraft_model::find_shape(v, id))
            .and_then(|x| if let ShapeKind::Table(t) = &x.kind { Some((t.n_rows(), t.n_cols())) } else { None });
        if let (Some(t), Some((nr, nc))) = (sel.text.as_mut(), dims)
            && let Some((r, c)) = t.cell
            && (r >= nr || c >= nc)
        {
            t.cell = Some((r.min(nr.saturating_sub(1)), c.min(nc.saturating_sub(1))));
            t.anchor = (0, 0);
            t.caret = (0, 0);
        }
    })?;
    Ok(r)
}

fn rows_cols(s: &mut Session, p: &Value, op: &str) -> Result<Value> {
    let (r, c) = at(s, p);
    let op = op.to_string();
    with_table(s, p, "table", move |t| {
        match op.as_str() {
            "rowAbove" => t.insert_row(r),
            "rowBelow" => t.insert_row(r.saturating_add(1)),
            "colLeft" => t.insert_col(c),
            "colRight" => t.insert_col(c.saturating_add(1)),
            "delRow" => {
                if !t.delete_row(r) {
                    return Err(bad("table.deleteRow", "can't delete the last row"));
                }
            }
            _ => {
                if !t.delete_col(c) {
                    return Err(bad("table.deleteColumn", "can't delete the last column"));
                }
            }
        }
        Ok(())
    })
}

fn merge(s: &mut Session, p: &Value) -> Result<Value> {
    let sel = s.doc()?.selection.cells;
    let from = cell_param(p, "from").or(sel.map(|c| c.1)).ok_or_else(|| bad("table.merge", "missing `from`"))?;
    let to = cell_param(p, "to").or(sel.map(|c| c.2)).ok_or_else(|| bad("table.merge", "missing `to`"))?;
    with_table(
        s,
        p,
        "table.merge",
        move |t| if t.merge(from.0, from.1, to.0, to.1) { Ok(()) } else { Err(bad("table.merge", "can't merge those cells")) },
    )
}

fn split(s: &mut Session, p: &Value) -> Result<Value> {
    let cell = cell_param(p, "cell").unwrap_or_else(|| at(s, p));
    with_table(s, p, "table.split", move |t| if t.split(cell.0, cell.1) { Ok(()) } else { Err(bad("table.split", "that cell isn't merged")) })
}

fn style(s: &mut Session, p: &Value) -> Result<Value> {
    let st = str_param(p, "style").ok_or_else(|| bad("table.style", "missing `style`"))?.to_string();
    with_table(s, p, "table.style", move |t| {
        t.style = st.clone();
        Ok(())
    })
}

fn options(s: &mut Session, p: &Value) -> Result<Value> {
    with_table(s, p, "table.options", |t| {
        if let Some(v) = bool_param(p, "headerRow") {
            t.first_row = v;
        }
        if let Some(v) = bool_param(p, "totalRow") {
            t.last_row = v;
        }
        if let Some(v) = bool_param(p, "bandedRows") {
            t.band_row = v;
        }
        if let Some(v) = bool_param(p, "firstColumn") {
            t.first_col = v;
        }
        if let Some(v) = bool_param(p, "lastColumn") {
            t.last_col = v;
        }
        if let Some(v) = bool_param(p, "bandedColumns") {
            t.band_col = v;
        }
        Ok(())
    })
}

/// The cells a command acts on: `cells`, else the selected cell range, else the cell holding the
/// caret, else every cell.
fn target_cells(s: &Session, p: &Value) -> Result<impl Fn(&Table) -> Vec<(usize, usize)> + use<>> {
    let cells: Option<Vec<(usize, usize)>> = p
        .get("cells")
        .and_then(Value::as_array)
        .map(|a| a.iter().take(100_000).filter_map(|c| Some((c.get(0)?.as_u64()? as usize, c.get(1)?.as_u64()? as usize))).collect());
    let st = s.doc()?;
    // A selected range grows over the merged cells it touches; with only a caret, the caret's cell is used.
    let sel = st.selection.cells.map(|(_, a, b)| (a, b, true)).or_else(|| st.selection.text.as_ref().and_then(|t| t.cell).map(|c| (c, c, false)));
    Ok(move |t: &Table| match (&cells, sel) {
        (Some(c), _) => c.clone(),
        (None, Some((a, b, range))) => {
            let (a, b) = if range { cell_block(t, a, b) } else { (a, b) };
            // The selection can name cells outside the table; only walk the ones that exist.
            let (r1, c1) = (a.0.max(b.0).min(t.n_rows().saturating_sub(1)), a.1.max(b.1).min(t.n_cols().saturating_sub(1)));
            (a.0.min(b.0)..=r1).flat_map(|r| (a.1.min(b.1)..=c1).map(move |c| (r, c))).collect()
        }
        _ => (0..t.n_rows()).flat_map(|r| (0..t.n_cols()).map(move |c| (r, c))).collect(),
    })
}

fn cell_fill(s: &mut Session, p: &Value) -> Result<Value> {
    let fill = if bool_or(p, "none", false) { Some(deckcraft_model::Fill::None) } else { color_param(p, "color").map(deckcraft_model::Fill::solid) };
    let targets = target_cells(s, p)?;
    with_table(s, p, "table.cellFill", move |t| {
        for (r, c) in targets(t) {
            if let Some(cell) = t.cell_mut(r, c) {
                cell.fill.clone_from(&fill);
            }
        }
        Ok(())
    })
}

const LEFT: usize = 0;
const RIGHT: usize = 1;
const TOP: usize = 2;
const BOTTOM: usize = 3;

fn borders(s: &mut Session, p: &Value) -> Result<Value> {
    use deckcraft_model::Line;
    let which = str_param(p, "which").unwrap_or("all").to_string();
    const WHICH: [&str; 12] = ["all", "outside", "inside", "insideH", "insideV", "top", "bottom", "left", "right", "diagDown", "diagUp", "none"];
    if !WHICH.contains(&which.as_str()) {
        return Err(bad("table.borders", format!("unknown `which` {which:?}; one of {}", WHICH.join(", "))));
    }
    let color = color_param(p, "color").unwrap_or_else(|| deckcraft_model::ColorRef::rgb(deckcraft_color::Rgba::rgb(0, 0, 0)));
    let width = f64_param(p, "width").filter(|w| w.is_finite()).unwrap_or(1.0).clamp(0.0, 100.0);
    let mut line = Line::solid(color, width);
    if let Some(d) = str_param(p, "dash").filter(|d| *d != "solid") {
        if !deckcraft_model::style::Dash::PRESETS.contains(&d) {
            return Err(bad("table.borders", format!("unknown dash {d:?}; one of {}", deckcraft_model::style::Dash::PRESETS.join(", "))));
        }
        line.dash = Some(deckcraft_model::style::Dash::Preset { name: d.to_string() });
    }
    // "none" clears to no line, so the table style's own lines don't show through either.
    let line = if which == "none" { Line::none() } else { line };
    let targets = target_cells(s, p)?;
    with_table(s, p, "table.borders", move |t| {
        let list = targets(t);
        let (Some(r0), Some(r1), Some(c0), Some(c1)) =
            (list.iter().map(|x| x.0).min(), list.iter().map(|x| x.0).max(), list.iter().map(|x| x.1).min(), list.iter().map(|x| x.1).max())
        else {
            return Ok(());
        };
        let (rows, cols) = (t.n_rows(), t.n_cols());
        // Set one side of a cell and the facing side of its neighbour, as PowerPoint does.
        let set = |t: &mut Table, r: usize, c: usize, side: usize| {
            if let Some(cell) = t.cell_mut(r, c) {
                cell.borders[side] = Some(line.clone());
            }
            let n = match side {
                LEFT => c.checked_sub(1).map(|c| (r, c, RIGHT)),
                RIGHT => (c + 1 < cols).then_some((r, c + 1, LEFT)),
                TOP => r.checked_sub(1).map(|r| (r, c, BOTTOM)),
                _ => (r + 1 < rows).then_some((r + 1, c, TOP)),
            };
            if let Some((r, c, side)) = n
                && let Some(cell) = t.cell_mut(r, c)
            {
                cell.borders[side] = Some(line.clone());
            }
        };
        for (r, c) in list {
            if r < r0 || r > r1 || c < c0 || c > c1 {
                continue;
            }
            let outer = [c == c0, c == c1, r == r0, r == r1];
            for side in [LEFT, RIGHT, TOP, BOTTOM] {
                let horizontal = side == TOP || side == BOTTOM;
                let on = match which.as_str() {
                    "all" | "none" => true,
                    "outside" => outer[side],
                    "inside" => !outer[side],
                    "insideH" => horizontal && !outer[side],
                    "insideV" => !horizontal && !outer[side],
                    "top" => side == TOP && outer[side],
                    "bottom" => side == BOTTOM && outer[side],
                    "left" => side == LEFT && outer[side],
                    "right" => side == RIGHT && outer[side],
                    _ => false,
                };
                if on {
                    set(t, r, c, side);
                }
            }
            if let Some(cell) = t.cell_mut(r, c) {
                match which.as_str() {
                    "diagDown" => cell.diag_down = Some(line.clone()),
                    "diagUp" => cell.diag_up = Some(line.clone()),
                    "none" => {
                        cell.diag_down = None;
                        cell.diag_up = None;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    })
}

fn cell_margins(s: &mut Session, p: &Value) -> Result<Value> {
    // PowerPoint's presets (left/right, top/bottom).
    let base = match str_param(p, "preset") {
        Some("none") => [0.0; 4],
        Some("narrow") => [3.6; 4],
        Some("wide") => [14.4, 14.4, 14.4, 14.4],
        Some("normal") | None => [7.2, 7.2, 3.6, 3.6],
        Some(o) => return Err(bad("table.cellMargins", format!("unknown preset {o:?}; one of normal, none, narrow, wide"))),
    };
    let side = |k: &str, i: usize| f64_param(p, k).filter(|v| v.is_finite()).map_or(base[i], |v| v.clamp(0.0, 400.0));
    let m = [side("left", 0), side("right", 1), side("top", 2), side("bottom", 3)];
    let targets = target_cells(s, p)?;
    with_table(s, p, "table.cellMargins", move |t| {
        for (r, c) in targets(t) {
            if let Some(cell) = t.cell_mut(r, c) {
                cell.margins = Some(m);
            }
        }
        Ok(())
    })
}

fn cell_align(s: &mut Session, p: &Value) -> Result<Value> {
    use deckcraft_model::text::Anchor;
    let a = match str_param(p, "anchor").unwrap_or("top") {
        "middle" | "center" | "ctr" => Anchor::Middle,
        "bottom" | "b" => Anchor::Bottom,
        "top" | "t" => Anchor::Top,
        o => return Err(bad("table.cellAlign", format!("unknown anchor {o:?}; one of top, middle, bottom"))),
    };
    let targets = target_cells(s, p)?;
    with_table(s, p, "table.cellAlign", move |t| {
        for (r, c) in targets(t) {
            if let Some(cell) = t.cell_mut(r, c) {
                cell.anchor = Some(a);
            }
        }
        Ok(())
    })
}

fn distribute(s: &mut Session, p: &Value, rows: bool) -> Result<Value> {
    with_table(s, p, "table.distribute", move |t| {
        if rows {
            let h = t.height() / t.n_rows().max(1) as f64;
            t.rows.iter_mut().for_each(|r| r.height = h);
        } else {
            let w = t.width() / t.n_cols().max(1) as f64;
            t.cols.iter_mut().for_each(|c| *c = w);
        }
        Ok(())
    })
}

fn column_width(s: &mut Session, p: &Value) -> Result<Value> {
    let (_, c) = at(s, p);
    let w = f64_param(p, "width").ok_or_else(|| bad("table.columnWidth", "missing `width`"))?.clamp(4.0, 4000.0);
    let keep_total = bool_or(p, "keepTotal", false);
    with_table(s, p, "table.columnWidth", move |t| {
        // Keeping the table's width, the column to the right gives or takes the difference (dragging
        // an inner border). The last column has none: the table gets wider or narrower.
        let pair = t.cols.get(c).zip(t.cols.get(c.saturating_add(1))).map(|(a, b)| a.max(0.0) + b.max(0.0)).filter(|x| x.is_finite());
        match pair {
            Some(pair) if keep_total => {
                let min = MIN_COLUMN_WIDTH.min(pair / 2.0);
                let w = w.max(min).min(pair - min);
                if let Some(x) = t.cols.get_mut(c) {
                    *x = w;
                }
                if let Some(x) = t.cols.get_mut(c.saturating_add(1)) {
                    *x = pair - w;
                }
            }
            _ => {
                if let Some(x) = t.cols.get_mut(c) {
                    *x = w;
                }
            }
        }
        Ok(())
    })
}

fn row_height(s: &mut Session, p: &Value) -> Result<Value> {
    let (r, _) = at(s, p);
    let h = f64_param(p, "height").ok_or_else(|| bad("table.rowHeight", "missing `height`"))?.clamp(4.0, 4000.0);
    with_table(s, p, "table.rowHeight", move |t| {
        if let Some(x) = t.rows.get_mut(r) {
            x.height = h;
        }
        Ok(())
    })
}

fn select_cells(s: &mut Session, p: &Value) -> Result<Value> {
    let id = match id_param(p, "id", "table.selectCells")? {
        Some(i) => i,
        None => s.doc()?.selection.shapes.first().copied().ok_or_else(|| bad("table.selectCells", "select a table"))?,
    };
    let from = cell_param(p, "from").unwrap_or((0, 0));
    let to = cell_param(p, "to").unwrap_or(from);
    s.select(|_, sel| {
        sel.cells = Some((id, from, to));
        sel.shapes = vec![id];
        sel.text = None;
    })?;
    ok()
}

/// The cell range from `a` to `b` in `t`, grown until every merged cell it touches lies inside
/// it, as (the corner on `a`'s side, the corner on `b`'s side). Cells past the table's edge are
/// clamped to it.
pub fn cell_block(t: &Table, a: (usize, usize), b: (usize, usize)) -> ((usize, usize), (usize, usize)) {
    let (nr, nc) = (t.n_rows().max(1), t.n_cols().max(1));
    let (a, b) = ((a.0.min(nr - 1), a.1.min(nc - 1)), (b.0.min(nr - 1), b.1.min(nc - 1)));
    let (mut r0, mut r1, mut c0, mut c1) = (a.0.min(b.0), a.0.max(b.0), a.1.min(b.1), a.1.max(b.1));
    // Each pass takes in the merged cells that overlap the block; stop when one adds nothing.
    for _ in 0..nr + nc {
        let mut grown = false;
        for (r, row) in t.rows.iter().enumerate() {
            for (c, cell) in row.cells.iter().enumerate() {
                if cell.is_covered() {
                    continue;
                }
                // Spans come from the file; the additions saturate so that they cannot overflow on 32-bit targets.
                let end = |at: usize, span: u32, n: usize| at.saturating_add((span.max(1) - 1) as usize).min(n - 1);
                let (er, ec) = (end(r, cell.row_span, nr), end(c, cell.grid_span, nc));
                let overlaps = r <= r1 && er >= r0 && c <= c1 && ec >= c0;
                if overlaps && (r < r0 || er > r1 || c < c0 || ec > c1) {
                    (r0, r1, c0, c1) = (r0.min(r), r1.max(er), c0.min(c), c1.max(ec));
                    grown = true;
                }
            }
        }
        if !grown {
            break;
        }
    }
    let pick = |lo: usize, hi: usize, first: bool| if first { lo } else { hi };
    let from = (pick(r0, r1, a.0 <= b.0), pick(c0, c1, a.1 <= b.1));
    let to = (pick(r0, r1, a.0 > b.0), pick(c0, c1, a.1 > b.1));
    (from, to)
}

/// The selected cell range of table `id` (`t`), grown over merged cells, as its top-left and
/// bottom-right cells. `None` when no range of that table is selected.
pub fn selected_range(
    cells: Option<(deckcraft_model::ShapeId, (usize, usize), (usize, usize))>,
    id: deckcraft_model::ShapeId,
    t: &Table,
) -> Option<((usize, usize), (usize, usize))> {
    let (_, a, b) = cells.filter(|c| c.0 == id)?;
    let (x, y) = cell_block(t, a, b);
    Some(((x.0.min(y.0), x.1.min(y.1)), (x.0.max(y.0), x.1.max(y.1))))
}

/// Whether cell (`r`, `c`) lies in `range` (from [`selected_range`]); without a range every cell does.
pub fn in_range(range: Option<((usize, usize), (usize, usize))>, r: usize, c: usize) -> bool {
    range.is_none_or(|((r0, c0), (r1, c1))| (r0..=r1).contains(&r) && (c0..=c1).contains(&c))
}

/// Shift+arrow with a cell range selected: the cell the range was extended to moves by (`dr`,
/// `dc`), on past a merged cell until the range changes. The cell it started from stays. Returns
/// false when no cell range of the selected table is selected.
pub(crate) fn extend_selected_cells(s: &mut Session, dr: isize, dc: isize) -> Result<bool> {
    let st = s.doc()?;
    let Some((id, from, to)) = st.selection.cells.filter(|c| st.selection.shapes == [c.0]) else { return Ok(false) };
    let Some(ShapeKind::Table(t)) = st.shape(id).map(|sh| &sh.kind) else { return Ok(false) };
    let (nr, nc) = (t.n_rows(), t.n_cols());
    let old = cell_block(t, from, to);
    let mut end = to;
    while cell_block(t, from, end) == old {
        let r = end.0.checked_add_signed(dr).filter(|r| *r < nr);
        let c = end.1.checked_add_signed(dc).filter(|c| *c < nc);
        let (Some(r), Some(c)) = (r, c) else { break };
        end = (r, c);
    }
    s.select(|_, sel| sel.cells = Some((id, from, end)))?;
    Ok(true)
}

/// The text of the selected range of cells, for Copy and Cut: a tab between the cells of a row
/// and a line per row (a merged cell once, where it starts). `None` without a cell range of the
/// selected table.
pub(crate) fn selected_cells_text(st: &DocState) -> Option<String> {
    let id = st.selection.cells.filter(|c| st.selection.shapes == [c.0])?.0;
    let ShapeKind::Table(t) = &st.shape(id)?.kind else { return None };
    let ((r0, c0), (r1, c1)) = selected_range(st.selection.cells, id, t)?;
    let rows = (r0..=r1).map(|r| {
        let cells = (c0..=c1).filter_map(|c| t.cell(r, c).filter(|x| !x.is_covered()).map(|x| x.text.text()));
        cells.collect::<Vec<_>>().join("\t")
    });
    Some(rows.collect::<Vec<_>>().join("\n"))
}

/// Delete with a cell range selected: the text in those cells goes and the cells stay, each
/// keeping its first paragraph's formatting.
pub(crate) fn clear_selected_cells(s: &mut Session) -> Result<Value> {
    let Some((id, ..)) = s.doc()?.selection.cells else { return ok() };
    let cells = s.doc()?.selection.cells;
    with_table(s, &json!({"id": id}), "edit.delete", move |t| {
        let range = selected_range(cells, id, t);
        for (r, row) in t.rows.iter_mut().enumerate() {
            for (c, cell) in row.cells.iter_mut().enumerate() {
                if in_range(range, r, c) {
                    deckcraft_model::edit::delete(&mut cell.text, (0, 0), (usize::MAX, usize::MAX));
                }
            }
        }
        Ok(())
    })
}

fn point_param(p: &Value, key: &str) -> Option<(f64, f64)> {
    let a = p.get(key)?;
    let (x, y) = (a.get(0)?.as_f64()?, a.get(1)?.as_f64()?);
    (x.is_finite() && y.is_finite()).then_some((x, y))
}

/// The table under slide point `at` (frontmost), or the one named by `id`, with its origin.
pub(crate) fn table_at(s: &Session, p: &Value, at: (f64, f64)) -> Option<(deckcraft_model::ShapeId, (f64, f64))> {
    let st = s.doc().ok()?;
    // A malformed id names no table (never the one under the pointer instead).
    let want = id_param(p, "id", "table.draw").ok()?;
    st.shapes().iter().rev().find_map(|sh| {
        let x = sh.xfrm?;
        let ShapeKind::Table(t) = &sh.kind else { return None };
        let inside = at.0 >= x.x && at.1 >= x.y && at.0 <= x.x + t.width() && at.1 <= x.y + t.height();
        (want.map_or(inside, |w| w == sh.id)).then_some((sh.id, (x.x, x.y)))
    })
}

fn draw_line(s: &mut Session, p: &Value) -> Result<Value> {
    let from = point_param(p, "from").ok_or_else(|| bad("table.drawLine", "missing `from`"))?;
    let to = point_param(p, "to").ok_or_else(|| bad("table.drawLine", "missing `to`"))?;
    let (id, o) = table_at(s, p, from).ok_or_else(|| bad("table.drawLine", "no table there"))?;
    let (a, b) = ((from.0 - o.0, from.1 - o.1), (to.0 - o.0, to.1 - o.1));
    with_table(s, &json!({"id": id.0}), "table.drawLine", move |t| {
        if crate::drawtable::draw_line(t, a, b) { Ok(()) } else { Err(bad("table.drawLine", "the line doesn't divide any cell")) }
    })
}

fn erase_line(s: &mut Session, p: &Value) -> Result<Value> {
    let at = point_param(p, "at").ok_or_else(|| bad("table.eraseLine", "missing `at`"))?;
    let tol = p.get("tol").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(6.0).clamp(0.5, 50.0);
    let (id, o) = table_at(s, p, at).ok_or_else(|| bad("table.eraseLine", "no table there"))?;
    let q = (at.0 - o.0, at.1 - o.1);
    with_table(s, &json!({"id": id.0}), "table.eraseLine", move |t| {
        if crate::drawtable::erase_at(t, q, tol) { Ok(()) } else { Err(bad("table.eraseLine", "no border to erase there")) }
    })
}
