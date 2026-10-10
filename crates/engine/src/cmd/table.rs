//! Table Design and Table Layout tabs.

use deckcraft_model::{ShapeKind, Table};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

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
        cmd!("table.split", "Split Cells", ["Table Layout", "Merge"], None, "{cell: [r, c], id?}", has_selection, split),
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
            "{headerRow?, totalRow?, bandedRows?, firstColumn?, lastColumn?, bandedColumns?: bool, id?}",
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
        cmd!("table.distributeRows", "Distribute Rows", ["Table Layout", "Cell Size"], None, "{id?}", has_selection, |s, p| distribute(s, p, true)),
        cmd!("table.distributeColumns", "Distribute Columns", ["Table Layout", "Cell Size"], None, "{id?}", has_selection, |s, p| distribute(
            s, p, false
        )),
        cmd!("table.columnWidth", "Width", ["Table Layout", "Cell Size"], None, "{col, width: pt, id?}", has_selection, column_width),
        cmd!("table.rowHeight", "Height", ["Table Layout", "Cell Size"], None, "{row, height: pt, id?}", has_selection, row_height),
        cmd!(noundo "table.selectCells", "Select Cells", [], None, "{from: [r,c], to: [r,c], id?}", has_selection, select_cells),
    ]
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
            "rowBelow" => t.insert_row(r + 1),
            "colLeft" => t.insert_col(c),
            "colRight" => t.insert_col(c + 1),
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

fn cell_fill(s: &mut Session, p: &Value) -> Result<Value> {
    let fill = if bool_or(p, "none", false) { Some(deckcraft_model::Fill::None) } else { color_param(p, "color").map(deckcraft_model::Fill::solid) };
    let cells: Option<Vec<(usize, usize)>> = p
        .get("cells")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|c| Some((c.get(0)?.as_u64()? as usize, c.get(1)?.as_u64()? as usize))).collect());
    let sel = s.doc()?.selection.cells;
    with_table(s, p, "table.cellFill", move |t| {
        let list: Vec<(usize, usize)> = match (&cells, sel) {
            (Some(c), _) => c.clone(),
            (None, Some((_, a, b))) => (a.0.min(b.0)..=a.0.max(b.0)).flat_map(|r| (a.1.min(b.1)..=a.1.max(b.1)).map(move |c| (r, c))).collect(),
            _ => (0..t.n_rows()).flat_map(|r| (0..t.n_cols()).map(move |c| (r, c))).collect(),
        };
        for (r, c) in list {
            if let Some(cell) = t.cell_mut(r, c) {
                cell.fill.clone_from(&fill);
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
    with_table(s, p, "table.columnWidth", move |t| {
        if let Some(x) = t.cols.get_mut(c) {
            *x = w;
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
