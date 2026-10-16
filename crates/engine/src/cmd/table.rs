//! Table Design and Table Layout tabs.

use deckcraft_geom::{Affine, Point, Rect};
use deckcraft_model::{Shape, ShapeKind, Table};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

use super::*;
use crate::{DocState, Result, Session};

#[path = "../table_content_commands.rs"]
mod content;

pub fn specs() -> Vec<CommandSpec> {
    let mut commands = vec![
        cmd!(
            "table.setCellValue",
            "Set Cell Value",
            ["Table Layout"],
            None,
            "{value, display, row?: index, col?: index, id?, path?} — explicit typed constant and its cached display; omitted coordinates require the edited cell",
            has_selection,
            set_cell_value
        ),
        cmd!(
            "table.setCellFormula",
            "Set Cell Formula",
            ["Table Layout"],
            None,
            "{expression: string|null, row?: index, col?: index, id?, path?, profile?: exact-decimal-v2|exact-decimal-v1} — v2 is default; v1 retains legacy arithmetic; null clears to literal display",
            has_selection,
            set_cell_formula
        ),
        cmd!(
            "table.recalculate",
            "Recalculate Table",
            ["Table Layout"],
            None,
            "{id?} — recalculate the selected table atomically",
            has_selection,
            recalculate
        ),
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
        cmd!(noundo "table.selectCells", "Select Cells", [], None, "{from: [r,c], to: [r,c], id?, path?} — endpoints inside the leaf grid; range may include covered cells", has_selection, select_cells),
    ];
    commands.extend(content::specs());
    commands
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
    let x = sh.xfrm.unwrap_or_else(|| {
        super::text::ctx_for(&st.doc, &st.selection)
            .map(|ctx| deckcraft_model::resolve::xfrm(&ctx, sh))
            .unwrap_or_else(|| xfrm_of(&st.doc, &st.selection, sh))
    });
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
    with_table_calculated(s, p, cmd, |table, _| f(table))
}

fn with_table_calculated(
    s: &mut Session,
    p: &Value,
    cmd: &str,
    f: impl Fn(&mut Table, &mut deckcraft_model::table::FormulaBudget) -> Result<()>,
) -> Result<Value> {
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
    let path = path_for(s, p, id, cmd)?;
    let r = s.edit_calculated(|doc, selection, budget| {
        let transform = super::current_shapes(doc, selection)
            .and_then(|shapes| deckcraft_model::find_shape(shapes, id))
            .map(|shape| {
                if matches!(cmd, "table.drawLine" | "table.eraseLine") {
                    super::text::ctx_for(doc, selection)
                        .map(|ctx| deckcraft_model::resolve::xfrm(&ctx, shape))
                        .unwrap_or_else(|| xfrm_of(doc, selection, shape))
                } else {
                    xfrm_of(doc, selection, shape)
                }
            })
            .ok_or_else(|| bad(cmd, "no such table"))?;
        let shapes = crate::shapes_mut(doc, selection).ok_or_else(|| bad(cmd, "no shape owner"))?;
        let shape = deckcraft_model::find_shape_mut(shapes, id).ok_or_else(|| bad(cmd, "no such table"))?;
        if shape.xfrm.is_none() {
            shape.xfrm = Some(transform);
        }
        let ShapeKind::Table(root) = &mut shape.kind else { return Err(bad(cmd, "not a table")) };
        let table = root.at_path_mut(&path)?;
        deckcraft_model::table::validate_constant_grid(table)?;
        f(table, budget)?;
        if !table.has_formulas() {
            table.normalize();
        }
        if path.is_empty()
            && !matches!(cmd, "table.drawLine" | "table.eraseLine")
            && let Some(transform) = shape.xfrm.as_mut()
        {
            transform.w = table.width();
            transform.h = table.height();
        }
        // Merge moves blocks between cell-local identity namespaces and may
        // renumber them. Reset the old text owner atomically with the document:
        // a syntactically valid descendant path can now name another child.
        if matches!(cmd, "table.merge" | "table.drawLine" | "table.eraseLine")
            && selection.table_owner == Some(id)
            && selection.table_path.0.starts_with(&path.0)
        {
            selection.table_path = path.clone();
            selection.cell_block = None;
            selection.text = None;
            selection.cells = None;
        }
        Ok(Value::from(1))
    })?;
    // The edited cell may have moved out of range.
    s.select(|doc, sel| {
        let dims = super::current_shapes(doc, sel)
            .and_then(|v| deckcraft_model::find_shape(v, id))
            .and_then(|x| if let ShapeKind::Table(t) = &x.kind { t.at_path(&path).ok().map(|t| (t.n_rows(), t.n_cols())) } else { None });
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

fn path_for(s: &Session, p: &Value, id: deckcraft_model::ShapeId, cmd: &str) -> Result<deckcraft_model::table::TablePath> {
    let path = if let Some(raw) = p.get("path") {
        let array = raw.as_array().ok_or_else(|| bad(cmd, "`path` must be an array"))?;
        if array.len() > deckcraft_model::table::MAX_NESTED_TABLE_DEPTH {
            return Err(bad(cmd, "nested-table path exceeds depth limit"));
        }
        serde_json::from_value(raw.clone()).map_err(|_| bad(cmd, "invalid nested-table path"))?
    } else if s.doc()?.selection.table_owner == Some(id) {
        s.doc()?.selection.table_path.clone()
    } else {
        Default::default()
    };
    path.validate()?;
    Ok(path)
}

/// Assign an explicit constant and display atomically; text editing never guesses a type.
fn set_cell_value(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "table.setCellValue";
    let value = deckcraft_model::table::CellValue::deserialize(p.get("value").ok_or_else(|| bad(CMD, "missing `value`"))?)
        .map_err(|e| bad(CMD, format!("invalid `value`: {e}")))?;
    let display = str_param(p, "display").ok_or_else(|| bad(CMD, "missing `display`"))?;
    if display.len() > 64 * 1024 || display.split('\n').take(1025).count() > 1024 {
        return Err(bad(CMD, "display exceeds cell constant size limit"));
    }
    let st = s.doc()?;
    let edited = st.selection.text.as_ref().filter(|t| !t.notes && t.cell.is_some());
    let id = id_param(p, "id", CMD)?
        .or(edited.map(|t| t.shape))
        .or_else(|| st.selection.shapes.first().copied())
        .ok_or_else(|| bad(CMD, "select a table or supply `id`"))?;
    let path = path_for(s, p, id, CMD)?;
    let default = edited.filter(|t| t.shape == id).and_then(|t| t.cell);
    let coordinate = |key: &str, fallback: Option<usize>| -> Result<usize> {
        if p.get(key).is_some() {
            usize_param(p, key).ok_or_else(|| bad(CMD, format!("invalid `{key}`")))
        } else {
            fallback.ok_or_else(|| bad(CMD, format!("missing `{key}` (or edit the target cell)")))
        }
    };
    let (row, col) = (coordinate("row", default.map(|c| c.0))?, coordinate("col", default.map(|c| c.1))?);
    let shape = st.shape(id).ok_or_else(|| bad(CMD, "no such table"))?;
    let ShapeKind::Table(table) = &shape.kind else { return Err(bad(CMD, "not a table")) };
    let table = table.at_path(&path)?;
    let cell = table.cell(row, col).ok_or_else(|| bad(CMD, "no such cell"))?;
    if cell.is_covered() || cell.grid_span.max(1) != 1 || cell.row_span.max(1) != 1 {
        return Err(bad(CMD, "typed assignment requires an unmerged visible cell"));
    }
    let formatted = cell.number_format.as_ref().map(|format| format.format_value(&value)).transpose()?;
    let body = content::body_with_display(&cell.text, formatted.as_ref().map_or(display, |value| value.text.as_str()));
    let data = Arc::new(deckcraft_model::table::CellConstant::from_display(value, &body)?);
    s.edit_calculated(|doc, sel, budget| {
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad(CMD, "no shape owner"))?;
        let shape = deckcraft_model::find_shape_mut(shapes, id).ok_or_else(|| bad(CMD, "no such table"))?;
        let ShapeKind::Table(table) = &mut shape.kind else { return Err(bad(CMD, "not a table")) };
        let table = table.at_path_mut(&path)?;
        let cell = table.cell_mut(row, col).ok_or_else(|| bad(CMD, "no such cell"))?;
        cell.text = body;
        cell.data = Some(data);
        cell.formula = None;
        if table.has_formulas() {
            budget.next_pass();
            table.recalculate_with_budget(budget)?;
        }
        // One explicit-assignment boundary scan, on the unpublished transaction copy.
        budget.next_pass();
        deckcraft_model::table::validate_constants_with_budget(doc, budget)?;
        Ok(())
    })?;
    ok()
}

fn strict_params(p: &Value, cmd: &str, allowed: &[&str]) -> Result<()> {
    let object = p.as_object().ok_or_else(|| bad(cmd, "parameters must be an object"))?;
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(bad(cmd, format!("unknown parameter `{key}`")));
    }
    if p.get("id").is_some_and(Value::is_null) {
        return Err(bad(cmd, "`id` must be a shape id"));
    }
    Ok(())
}

fn formula_target(s: &Session, p: &Value, cmd: &str) -> Result<deckcraft_model::ShapeId> {
    let st = s.doc()?;
    id_param(p, "id", cmd)?
        .or_else(|| st.selection.text.as_ref().filter(|t| !t.notes && t.cell.is_some()).map(|t| t.shape))
        .or(st.selection.cells.map(|cells| cells.0))
        .or_else(|| st.selection.shapes.first().copied())
        .ok_or_else(|| bad(cmd, "select a table or supply `id`"))
}

fn set_cell_formula(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "table.setCellFormula";
    strict_params(p, CMD, &["expression", "row", "col", "id", "path", "profile"])?;
    let profile = p
        .get("profile")
        .map(|v| v.as_str().ok_or_else(|| bad(CMD, "invalid formula `profile`")))
        .transpose()?
        .unwrap_or(deckcraft_model::table::FORMULA_PROFILE_V2);
    if !matches!(profile, deckcraft_model::table::FORMULA_PROFILE | deckcraft_model::table::FORMULA_PROFILE_V2) {
        return Err(bad(CMD, "unknown formula profile"));
    }
    let expression = match p.get("expression") {
        Some(Value::String(source)) => Some(source.as_str()),
        Some(Value::Null) => None,
        _ => return Err(bad(CMD, "`expression` must be a formula string or null")),
    };
    let id = formula_target(s, p, CMD)?;
    let path = path_for(s, p, id, CMD)?;
    let st = s.doc()?;
    let default = st.selection.text.as_ref().filter(|t| !t.notes && t.shape == id).and_then(|t| t.cell);
    let coordinate = |key: &str, fallback: Option<usize>| -> Result<usize> {
        if p.get(key).is_some() {
            usize_param(p, key).ok_or_else(|| bad(CMD, format!("invalid `{key}`")))
        } else {
            fallback.ok_or_else(|| bad(CMD, format!("missing `{key}` (or edit the target cell)")))
        }
    };
    let (row, col) = (coordinate("row", default.map(|c| c.0))?, coordinate("col", default.map(|c| c.1))?);
    let shape = st.shape(id).ok_or_else(|| bad(CMD, "no such table"))?;
    let ShapeKind::Table(table) = &shape.kind else { return Err(bad(CMD, "not a table")) };
    let table = table.at_path(&path)?;
    let cell = table.cell(row, col).ok_or_else(|| bad(CMD, "no such cell"))?;
    if cell.is_covered() || cell.grid_span.max(1) != 1 || cell.row_span.max(1) != 1 {
        return Err(bad(CMD, "formula assignment requires an unmerged visible cell"));
    }
    s.edit_calculated(|doc, sel, budget| {
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad(CMD, "no shape owner"))?;
        let shape = deckcraft_model::find_shape_mut(shapes, id).ok_or_else(|| bad(CMD, "no such table"))?;
        let ShapeKind::Table(table) = &mut shape.kind else { return Err(bad(CMD, "not a table")) };
        let table = table.at_path_mut(&path)?;
        if let Some(expression) = expression {
            budget.next_pass();
            if profile == deckcraft_model::table::FORMULA_PROFILE {
                table.set_cell_formula_with_budget(row, col, expression, budget)?;
            } else {
                table.set_cell_formula_v2_with_budget(row, col, expression, budget)?;
            }
        } else {
            let cell = table.cell_mut(row, col).ok_or_else(|| bad(CMD, "no such cell"))?;
            cell.formula = None;
            if table.has_formulas() {
                budget.next_pass();
                table.recalculate_with_budget(budget)?;
            }
        }
        budget.next_pass();
        deckcraft_model::table::validate_constants_with_budget(doc, budget)?;
        Ok(())
    })?;
    ok()
}

fn recalculate(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "table.recalculate";
    strict_params(p, CMD, &["id", "path"])?;
    let id = formula_target(s, p, CMD)?;
    let path = path_for(s, p, id, CMD)?;
    s.edit_calculated(|doc, sel, budget| {
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad(CMD, "no shape owner"))?;
        let shape = deckcraft_model::find_shape_mut(shapes, id).ok_or_else(|| bad(CMD, "no such table"))?;
        let ShapeKind::Table(table) = &mut shape.kind else { return Err(bad(CMD, "not a table")) };
        let table = table.at_path_mut(&path)?;
        budget.next_pass();
        table.recalculate_with_budget(budget)?;
        budget.next_pass();
        deckcraft_model::table::validate_constants_with_budget(doc, budget)?;
        Ok(())
    })?;
    ok()
}

fn rows_cols(s: &mut Session, p: &Value, op: &str) -> Result<Value> {
    let (r, c) = at(s, p);
    let columns = matches!(op, "colLeft" | "colRight" | "delCol");
    let delete = matches!(op, "delRow" | "delCol");
    let mut at = if columns { c } else { r };
    if matches!(op, "rowBelow" | "colRight") {
        at = at.checked_add(1).ok_or_else(|| bad("table", "coordinate overflow"))?;
    }
    let id = formula_target(s, p, "table")?;
    let path = path_for(s, p, id, "table")?;
    s.edit_calculated(|doc, selection, budget| {
        let shapes = crate::shapes_mut(doc, selection).ok_or_else(|| bad("table", "no shape owner"))?;
        let shape = deckcraft_model::find_shape_mut(shapes, id).ok_or_else(|| bad("table", "no such table"))?;
        let ShapeKind::Table(table) = &mut shape.kind else { return Err(bad("table", "not a table")) };
        table.at_path_mut(&path)?.edit_axis_with_budget(columns, at, delete, budget)?;
        // A direct caret belongs to a physical cell too. Keep it attached to
        // surviving content; deleting that owner exits editing rather than
        // silently redirecting typing into its neighbour/continuation block.
        if selection.table_owner == Some(id) && selection.table_path == path {
            if let Some(text) = selection.text.as_mut().filter(|text| text.shape == id)
                && let Some((row, col)) = text.cell
            {
                let coordinate = if columns { col } else { row };
                if delete && coordinate == at {
                    selection.text = None;
                    selection.cell_block = None;
                    selection.cells = None;
                } else {
                    let coordinate = if delete && coordinate > at {
                        coordinate - 1
                    } else if !delete && coordinate >= at {
                        coordinate.checked_add(1).ok_or_else(|| bad("table", "selection coordinate overflow"))?
                    } else {
                        coordinate
                    };
                    text.cell = Some(if columns { (row, coordinate) } else { (coordinate, col) });
                }
            }
            if let Some((_, from, to)) = selection.cells.as_mut().filter(|(owner, _, _)| *owner == id) {
                let length = if columns { table.at_path(&path)?.n_cols() } else { table.at_path(&path)?.n_rows() };
                for cell in [from, to] {
                    let coordinate = if columns { &mut cell.1 } else { &mut cell.0 };
                    if delete && *coordinate >= at {
                        *coordinate = if *coordinate == at { at.min(length.saturating_sub(1)) } else { *coordinate - 1 };
                    } else if !delete && *coordinate >= at {
                        *coordinate = coordinate.checked_add(1).ok_or_else(|| bad("table", "selection coordinate overflow"))?;
                    }
                }
            }
        }
        // Descendant selection paths use physical coordinates in the edited grid.
        if selection.table_owner == Some(id)
            && selection.table_path.0.starts_with(&path.0)
            && let Some(step) = selection.table_path.0.get_mut(path.0.len())
        {
            let coordinate = if columns { &mut step.col } else { &mut step.row };
            if delete && *coordinate == at {
                selection.table_path = path.clone();
                selection.text = None;
                selection.cell_block = None;
                selection.cells = None;
            } else if delete && *coordinate > at {
                *coordinate -= 1;
            } else if !delete && *coordinate >= at {
                *coordinate = coordinate.checked_add(1).ok_or_else(|| bad("table", "selection coordinate overflow"))?;
            }
        }
        Ok(Value::Null)
    })
}

fn merge(s: &mut Session, p: &Value) -> Result<Value> {
    let sel = s.doc()?.selection.cells;
    let from = cell_param(p, "from").or(sel.map(|c| c.1)).ok_or_else(|| bad("table.merge", "missing `from`"))?;
    let to = cell_param(p, "to").or(sel.map(|c| c.2)).ok_or_else(|| bad("table.merge", "missing `to`"))?;
    with_table_calculated(s, p, "table.merge", move |t, budget| {
        t.merge_with_budget(from, to, budget)?;
        Ok(())
    })
}

fn split(s: &mut Session, p: &Value) -> Result<Value> {
    let cell = cell_param(p, "cell").unwrap_or_else(|| at(s, p));
    with_table_calculated(s, p, "table.split", move |t, budget| {
        t.split_with_budget(cell.0, cell.1, budget)?;
        Ok(())
    })
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
    let sel = st.selection.cells.map(|(_, a, b)| (a, b)).or_else(|| st.selection.text.as_ref().and_then(|t| t.cell).map(|c| (c, c)));
    Ok(move |t: &Table| match (&cells, sel) {
        (Some(c), _) => c.clone(),
        (None, Some((a, b))) => {
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
        let pair = t.cols.get(c).zip(t.cols.get(c + 1)).map(|(a, b)| a.max(0.0) + b.max(0.0)).filter(|x| x.is_finite());
        match pair {
            Some(pair) if keep_total => {
                let min = MIN_COLUMN_WIDTH.min(pair / 2.0);
                let w = w.max(min).min(pair - min);
                if let Some(x) = t.cols.get_mut(c) {
                    *x = w;
                }
                if let Some(x) = t.cols.get_mut(c + 1) {
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
    let path = path_for(s, p, id, "table.selectCells")?;
    let shape = s.doc()?.shape(id).ok_or_else(|| bad("table.selectCells", "no such table"))?;
    let ShapeKind::Table(table) = &shape.kind else { return Err(bad("table.selectCells", "not a table")) };
    let leaf = table.at_path(&path)?;
    let from =
        if p.get("from").is_some() { cell_param(p, "from").ok_or_else(|| bad("table.selectCells", "invalid from coordinate"))? } else { (0, 0) };
    let to = if p.get("to").is_some() { cell_param(p, "to").ok_or_else(|| bad("table.selectCells", "invalid to coordinate"))? } else { from };
    // Endpoints must lie in the actual leaf grid; the rectangular range may
    // include covered physical cells, as merge and shading selections do.
    if leaf.cell(from.0, from.1).is_none() || leaf.cell(to.0, to.1).is_none() {
        return Err(bad("table.selectCells", "range endpoint is outside the selected table grid"));
    }
    s.select(|_, sel| {
        sel.cells = Some((id, from, to));
        sel.shapes = vec![id];
        sel.table_owner = Some(id);
        sel.table_path = path;
        sel.cell_block = None;
        sel.text = None;
    })?;
    ok()
}

fn point_param(p: &Value, key: &str) -> Option<(f64, f64)> {
    let a = p.get(key)?;
    let (x, y) = (a.get(0)?.as_f64()?, a.get(1)?.as_f64()?);
    (x.is_finite() && y.is_finite()).then_some((x, y))
}

/// The frontmost table's actual visible grid and affine, including grown rows,
/// group transforms and flips/rotation. Named owners may be outside the point.
pub(crate) fn table_at(s: &Session, p: &Value, at: (f64, f64)) -> Option<(deckcraft_model::ShapeId, TableFrame)> {
    let st = s.doc().ok()?;
    let want = id_param(p, "id", "table.draw").ok()?;
    let point = Point::new(at.0, at.1);
    fn find(
        st: &DocState,
        items: &[Shape],
        parent: Affine,
        want: Option<deckcraft_model::ShapeId>,
        point: Point,
        depth: usize,
    ) -> Option<(deckcraft_model::ShapeId, TableFrame)> {
        if depth > 31 {
            return None;
        }
        for shape in items.iter().rev() {
            let x = shape.xfrm.unwrap_or_else(|| xfrm_of(&st.doc, &st.selection, shape));
            if !x.is_finite() {
                continue;
            }
            if let ShapeKind::Group { children, child } = &shape.kind {
                let transform = parent * deckcraft_geom::group_child_affine(&x, child.rect());
                if transform.as_coeffs().iter().all(|n| n.is_finite())
                    && transform.determinant().is_finite()
                    && transform.determinant().abs() > 1e-12
                    && let Some(found) = find(st, children, transform, want, point, depth + 1)
                {
                    return Some(found);
                }
            } else if let ShapeKind::Table(table) = &shape.kind {
                if want.is_some_and(|id| id != shape.id)
                    || table.n_rows() > 1000
                    || table.n_cols() > 1000
                    || table.validate_content().is_err()
                    || deckcraft_model::table::validate_merged_grid(table).is_err()
                {
                    continue;
                }
                let Some(mut frame) = table_frame(st, shape) else {
                    continue;
                };
                frame.affine = parent * frame.affine;
                if !frame.affine.as_coeffs().iter().all(|n| n.is_finite())
                    || !frame.affine.determinant().is_finite()
                    || frame.affine.determinant().abs() <= 1e-12
                    || frame.heights.iter().any(|n| !n.is_finite() || *n < 0.)
                {
                    continue;
                }
                let local = frame.local(point);
                let inside = local.x >= 0. && local.y >= 0. && local.x <= frame.width() && local.y <= frame.height();
                if want == Some(shape.id) || inside {
                    return Some((shape.id, frame));
                }
            }
        }
        None
    }
    find(st, st.shapes(), Affine::IDENTITY, want, point, 0)
}

fn materialize_drawn_rows(table: &mut Table, frame: &TableFrame, command: &str) -> Result<()> {
    if frame.heights.len() != table.n_rows() || frame.heights.iter().any(|height| !height.is_finite() || *height < 0. || *height > 20_000.) {
        return Err(bad(command, "drawn row geometry exceeds table gesture limits"));
    }
    for (row, height) in table.rows.iter_mut().zip(&frame.heights) {
        row.height = *height;
    }
    Ok(())
}

fn draw_line(s: &mut Session, p: &Value) -> Result<Value> {
    let from = point_param(p, "from").ok_or_else(|| bad("table.drawLine", "missing `from`"))?;
    let to = point_param(p, "to").ok_or_else(|| bad("table.drawLine", "missing `to`"))?;
    let (id, frame) = table_at(s, p, from).ok_or_else(|| bad("table.drawLine", "no table there"))?;
    let a = frame.local(Point::new(from.0, from.1));
    let b = frame.local(Point::new(to.0, to.1));
    if ![a.x, a.y, b.x, b.y].iter().all(|n| n.is_finite()) {
        return Err(bad("table.drawLine", "invalid table transform"));
    }
    let (a, b) = ((a.x, a.y), (b.x, b.y));
    with_table_calculated(s, &json!({"id": id.0, "path": []}), "table.drawLine", move |t, budget| {
        materialize_drawn_rows(t, &frame, "table.drawLine")?;
        if crate::drawtable::draw_line_with_budget(t, a, b, budget) { Ok(()) } else { Err(bad("table.drawLine", "the line doesn't divide any cell")) }
    })
}

fn erase_line(s: &mut Session, p: &Value) -> Result<Value> {
    let at = point_param(p, "at").ok_or_else(|| bad("table.eraseLine", "missing `at`"))?;
    let tol = p.get("tol").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(6.0).clamp(0.5, 50.0);
    let (id, frame) = table_at(s, p, at).ok_or_else(|| bad("table.eraseLine", "no table there"))?;
    let q = frame.local(Point::new(at.0, at.1));
    if ![q.x, q.y].iter().all(|n| n.is_finite()) {
        return Err(bad("table.eraseLine", "invalid table transform"));
    }
    let q = (q.x, q.y);
    with_table_calculated(s, &json!({"id": id.0, "path": []}), "table.eraseLine", move |t, budget| {
        materialize_drawn_rows(t, &frame, "table.eraseLine")?;
        if crate::drawtable::erase_at_with_affine_budget(t, q, tol, frame.affine, budget) {
            Ok(())
        } else {
            Err(bad("table.eraseLine", "no border to erase there"))
        }
    })
}
